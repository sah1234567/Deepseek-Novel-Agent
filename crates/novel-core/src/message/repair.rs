use crate::message::missing_blocks::yield_missing_tool_result_blocks;
use crate::ChatMessage;
use std::collections::HashSet;

/// Caller context for tool-chain repair diagnostics (`RUST_LOG=novel_core=debug`).
#[derive(Debug, Clone, Copy)]
pub struct RepairTraceContext<'a> {
    pub label: &'a str,
    pub fork_run_id: Option<&'a str>,
    pub inner_turn: Option<u32>,
    pub session_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolChainGap {
    pub(crate) assistant_index: usize,
    pub(crate) tool_call_ids: Vec<String>,
    pub(crate) tool_names: Vec<String>,
}

pub(crate) fn scan_tool_chain_gaps(messages: &[ChatMessage]) -> Vec<ToolChainGap> {
    let mut gaps = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }
        let tool_calls = match &messages[i].tool_calls {
            Some(tcs) if !tcs.is_empty() => tcs,
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut seen = HashSet::new();
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = &messages[j].tool_call_id {
                seen.insert(id.clone());
            }
            j += 1;
        }
        let missing: Vec<_> = tool_calls
            .iter()
            .filter(|tc| !seen.contains(&tc.id))
            .collect();
        if !missing.is_empty() {
            gaps.push(ToolChainGap {
                assistant_index: i,
                tool_call_ids: missing.iter().map(|tc| tc.id.clone()).collect(),
                tool_names: missing.iter().map(|tc| tc.name.clone()).collect(),
            });
        }
        i = j;
    }
    gaps
}

/// Ensure tool messages match a preceding assistant `tool_calls` block (API requirement).
pub fn repair_tool_use_chain(messages: &mut Vec<ChatMessage>) {
    repair_tool_use_chain_traced(messages, None);
}

pub fn repair_tool_use_chain_traced(
    messages: &mut Vec<ChatMessage>,
    trace: Option<RepairTraceContext<'_>>,
) {
    let removed = remove_orphan_tool_messages(messages);
    let inserted = fill_missing_tool_results(messages, trace);
    if removed > 0 || inserted > 0 {
        tracing::debug!(
            removed_orphans = removed,
            inserted_stubs = inserted,
            label = trace.map(|t| t.label),
            fork_run_id = trace.and_then(|t| t.fork_run_id),
            inner_turn = trace.and_then(|t| t.inner_turn),
            "repaired tool_use chain"
        );
    }
}

fn remove_orphan_tool_messages(messages: &mut Vec<ChatMessage>) -> usize {
    let before = messages.len();
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "tool" {
            i += 1;
            continue;
        }
        let tool_id = messages[i].tool_call_id.clone();
        if !is_tool_call_valid_at(messages, i, tool_id.as_deref()) {
            tracing::warn!(
                tool_call_id = ?tool_id,
                index = i,
                "removing orphan tool message without preceding assistant tool_calls"
            );
            messages.remove(i);
            continue;
        }
        i += 1;
    }
    before.saturating_sub(messages.len())
}

fn is_tool_call_valid_at(messages: &[ChatMessage], tool_idx: usize, tool_id: Option<&str>) -> bool {
    let Some(tid) = tool_id else {
        return false;
    };
    let mut block_start = tool_idx;
    while block_start > 0 && messages[block_start - 1].role == "tool" {
        block_start -= 1;
    }
    if block_start == 0 {
        return false;
    }
    let assistant_idx = block_start - 1;
    if messages[assistant_idx].role != "assistant" {
        return false;
    }
    messages[assistant_idx]
        .tool_calls
        .as_ref()
        .is_some_and(|tcs| tcs.iter().any(|tc| tc.id == tid))
}

fn fill_missing_tool_results(
    messages: &mut Vec<ChatMessage>,
    trace: Option<RepairTraceContext<'_>>,
) -> usize {
    let mut inserted = 0usize;
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }
        let tool_calls = match &messages[i].tool_calls {
            Some(tcs) if !tcs.is_empty() => tcs.clone(),
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut seen = HashSet::new();
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = &messages[j].tool_call_id {
                seen.insert(id.clone());
            }
            j += 1;
        }
        let missing_records: Vec<_> = tool_calls
            .iter()
            .filter(|tc| !seen.contains(&tc.id))
            .collect();
        if !missing_records.is_empty() {
            let missing: Vec<&str> = missing_records.iter().map(|tc| tc.id.as_str()).collect();
            let missing_tool_names: Vec<&str> =
                missing_records.iter().map(|tc| tc.name.as_str()).collect();
            tracing::warn!(
                ?missing,
                ?missing_tool_names,
                assistant_index = i,
                message_count = messages.len(),
                label = trace.map(|t| t.label),
                fork_run_id = trace.and_then(|t| t.fork_run_id),
                inner_turn = trace.and_then(|t| t.inner_turn),
                session_id = trace.and_then(|t| t.session_id),
                "tool_chain_missing_results_repaired"
            );
            let assistant = messages[i].clone();
            let stubs = yield_missing_tool_result_blocks(
                &assistant,
                "Error: tool result was not recorded (session repaired)",
            );
            for stub in stubs {
                if stub
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| missing.contains(&id.as_str()))
                {
                    messages.insert(j, stub);
                    j += 1;
                    inserted += 1;
                }
            }
        }
        i = j;
    }
    inserted
}
