//! Read-cache persistence orchestration (hydrate / rebuild / flush / reconcile).

use crate::engine::EngineShared;
use crate::message::tool_pairs::collect_tool_use_result_pairs;
use crate::read_cache::cutoff::messages_replay_cutoff_chat;
use crate::{AgentError, ChatMessage};
use novel_state::{Database, ReadCacheAnchor, StoredMessage};
use novel_tools::{
    hydrate_read_file_cache_into, normalize_rel_path, read_cache_entry_from_json,
    read_cache_entry_to_json, rebuild_read_cache_from_pairs, ReadCacheEntry, ReadCacheReplayPair,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) fn read_cache_touch_callback(
    dirty: &Arc<Mutex<HashSet<PathBuf>>>,
) -> Arc<dyn Fn(PathBuf) + Send + Sync> {
    let dirty = Arc::clone(dirty);
    Arc::new(move |path| {
        if let Ok(mut guard) = dirty.lock() {
            guard.insert(path);
        }
    })
}

fn anchor_from_stored(stored: &[StoredMessage]) -> Option<(i32, i32)> {
    stored.last().map(|m| (m.turn_number, m.sequence))
}

fn anchor_matches(
    stored: &[StoredMessage],
    anchor: &ReadCacheAnchor,
    compaction_count: i32,
) -> bool {
    let Some((turn, seq)) = anchor_from_stored(stored) else {
        return false;
    };
    anchor.compaction_count == compaction_count
        && anchor.anchor_turn == turn
        && anchor.anchor_sequence == seq
}

fn pairs_from_messages(messages: &[ChatMessage], replay_from: usize) -> Vec<ReadCacheReplayPair> {
    collect_tool_use_result_pairs(messages, replay_from)
        .into_iter()
        .map(|p| ReadCacheReplayPair {
            tool_name: p.call.name.clone(),
            arguments: p.call.arguments.clone(),
            result_content: p.result.content.clone(),
        })
        .collect()
}

/// Strip Windows `\\?\` verbatim prefix if present, so canonicalized paths
/// (from `std::fs::canonicalize`) can still match a non-canonicalized `project_root`.
fn strip_verbatim_prefix(path: &Path) -> std::borrow::Cow<'_, Path> {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return std::borrow::Cow::Owned(PathBuf::from(rest));
    }
    std::borrow::Cow::Borrowed(path)
}

fn rel_path_for_cache_key(project_root: &Path, full: &Path) -> Option<String> {
    let full = strip_verbatim_prefix(full);
    let root = strip_verbatim_prefix(project_root);
    full.strip_prefix(root.as_ref())
        .ok()
        .map(|p| normalize_rel_path(&p.to_string_lossy()))
}

fn session_compaction_count(shared: &EngineShared) -> Result<i32, AgentError> {
    shared
        .session
        .db
        .get_compaction_count(&shared.session.id)
        .map_err(AgentError::from)
}

fn upsert_read_cache_entry(
    db: &Database,
    session_id: &str,
    project_root: &Path,
    full: &Path,
    entry: &ReadCacheEntry,
) -> Result<Option<String>, AgentError> {
    let Some(rel) = rel_path_for_cache_key(project_root, full) else {
        tracing::warn!(path = %full.display(), "read_cache upsert: path outside project_root, skipping");
        return Ok(None);
    };
    let json = read_cache_entry_to_json(entry).map_err(AgentError::from)?;
    db.upsert_session_read_cache_entry(session_id, &rel, &json)
        .map_err(AgentError::from)?;
    Ok(Some(rel))
}

fn persist_read_cache_anchor(
    db: &Database,
    session_id: &str,
    anchor_turn: i32,
    anchor_sequence: i32,
    compaction_count: i32,
) -> Result<(), AgentError> {
    db.set_read_cache_anchor(
        session_id,
        &ReadCacheAnchor {
            compaction_count,
            anchor_turn,
            anchor_sequence,
        },
    )
    .map_err(AgentError::from)
}

fn clear_read_cache_dirty(shared: &EngineShared) {
    if let Ok(mut dirty) = shared.read_cache_dirty_paths.lock() {
        dirty.clear();
    }
}

/// Resume: hydrate from SQLite when anchor matches, else rebuild from transcript slice.
pub(crate) fn try_restore_read_cache_on_resume(
    shared: &EngineShared,
    messages: &[ChatMessage],
    stored: &[StoredMessage],
) -> Result<(), AgentError> {
    let session_id = &shared.session.id;
    let db = &shared.session.db;
    let compaction_count = session_compaction_count(shared)?;

    let rows = db
        .list_session_read_cache(session_id)
        .map_err(AgentError::from)?;
    if let Some(anchor) = db
        .get_read_cache_anchor(session_id)
        .map_err(AgentError::from)?
    {
        if anchor_matches(stored, &anchor, compaction_count) && !rows.is_empty() {
            let mut parsed = Vec::with_capacity(rows.len());
            for (path, json) in rows {
                match read_cache_entry_from_json(&json) {
                    Ok(entry) => parsed.push((path, entry)),
                    Err(e) => {
                        tracing::warn!(path = %path, error = %e, "read_cache hydrate json failed");
                    }
                }
            }
            if !parsed.is_empty() {
                hydrate_read_file_cache_into(
                    &shared.read_file_cache,
                    &shared.session.project_root,
                    &parsed,
                );
                clear_read_cache_dirty(shared);
                return Ok(());
            }
        }
    }

    rebuild_and_reconcile_read_cache(shared, messages, stored)
}

/// Rebuild from replay slice and reconcile SQLite to match memory.
pub(crate) fn rebuild_and_reconcile_read_cache(
    shared: &EngineShared,
    messages: &[ChatMessage],
    stored: &[StoredMessage],
) -> Result<(), AgentError> {
    let replay_from = messages_replay_cutoff_chat(messages);
    let pairs = pairs_from_messages(messages, replay_from);
    rebuild_read_cache_from_pairs(
        &shared.read_file_cache,
        &shared.session.project_root,
        &shared.registry,
        &pairs,
    );

    let (turn, seq) = anchor_from_stored(stored).unwrap_or((0, 0));
    reconcile_session_read_cache(shared, turn, seq, session_compaction_count(shared)?)
}

pub(crate) fn reconcile_session_read_cache(
    shared: &EngineShared,
    anchor_turn: i32,
    anchor_sequence: i32,
    compaction_count: i32,
) -> Result<(), AgentError> {
    let session_id = &shared.session.id;
    let db = &shared.session.db;
    let project_root = &shared.session.project_root;

    let mut keep_paths = Vec::new();
    for item in shared.read_file_cache.iter() {
        if let Some(rel) =
            upsert_read_cache_entry(db, session_id, project_root, item.key(), item.value())?
        {
            keep_paths.push(rel);
        }
    }
    db.delete_session_read_cache_paths_not_in(session_id, &keep_paths)
        .map_err(AgentError::from)?;
    persist_read_cache_anchor(
        db,
        session_id,
        anchor_turn,
        anchor_sequence,
        compaction_count,
    )?;
    clear_read_cache_dirty(shared);
    Ok(())
}

/// After each LLM API batch: UPSERT only dirty paths using DashMap final state.
pub(crate) fn flush_dirty_read_cache_paths(
    shared: &EngineShared,
    anchor_turn: i32,
    anchor_sequence: i32,
) -> Result<(), AgentError> {
    let paths: Vec<PathBuf> = {
        let guard = shared
            .read_cache_dirty_paths
            .lock()
            .map_err(|_| AgentError::Validation("read_cache_dirty_paths lock poisoned".into()))?;
        guard.iter().cloned().collect()
    };
    if paths.is_empty() {
        return Ok(());
    }

    let session_id = &shared.session.id;
    let db = &shared.session.db;
    let project_root = &shared.session.project_root;
    let compaction_count = session_compaction_count(shared)?;

    for full in paths {
        let Some(entry) = shared.read_file_cache.get(&full) else {
            continue;
        };
        upsert_read_cache_entry(db, session_id, project_root, &full, entry.value())?;
    }

    persist_read_cache_anchor(
        db,
        session_id,
        anchor_turn,
        anchor_sequence,
        compaction_count,
    )?;
    clear_read_cache_dirty(shared);
    Ok(())
}

pub(crate) fn clear_read_file_cache_persisted(shared: &EngineShared) -> Result<(), AgentError> {
    shared.read_file_cache.clear();
    shared
        .session
        .db
        .clear_session_read_cache(&shared.session.id)
        .map_err(AgentError::from)?;
    shared
        .session
        .db
        .clear_read_cache_anchor(&shared.session.id)
        .map_err(AgentError::from)?;
    clear_read_cache_dirty(shared);
    Ok(())
}
