mod frozen;
mod memory;
mod progress;
mod skills;

pub use frozen::{
    load_frozen_static_from_metadata, persist_frozen_system_metadata,
    refresh_system_dynamic_context,
};
pub use skills::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
    parse_skill_reference_path,
};

use std::path::Path;

use novel_knowledge::KnowledgeStore;
use novel_state::Database;

use crate::context::system_prompt::DynamicContext;

/// Assemble dynamic system prompt sections (Progress, Memory, INDEX, AGENTS, Skill summaries).
pub fn build_dynamic_context(
    project_root: &Path,
    session_id: &str,
    db: &Database,
    agents_md: &str,
    skills_dir: &Path,
) -> DynamicContext {
    let store = KnowledgeStore::new(project_root);
    let index = store
        .read_file("knowledge/INDEX.md")
        .unwrap_or_else(|_| "（索引尚未创建）".into());
    let project_skills = project_root.join("skills");
    let skills = match novel_skills::load_skills_merged(&project_skills, skills_dir) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                skills_dir = %skills_dir.display(),
                %e,
                "failed to load skills for dynamic context"
            );
            Vec::new()
        }
    };
    let skill_summaries: Vec<(String, String)> = skills
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                novel_skills::format_skill_listing_description(&s.description, &s.when_to_use),
            )
        })
        .collect();
    DynamicContext {
        agents_md: agents_md.to_string(),
        knowledge_index: index.chars().take(2000).collect(),
        memory: memory::load_memory(project_root, 4096),
        progress: progress::load_progress(project_root, session_id, db),
        skill_summaries,
        workspace_path: project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf())
            .display()
            .to_string(),
    }
}
#[cfg(test)]
mod tests {
    fn format_invoked_skill_bodies(
        project_root: &Path,
        agent_skills_dir: &Path,
        invoked_skill_ids: &[String],
    ) -> String {
        format_activated_skill_block(project_root, agent_skills_dir, invoked_skill_ids, &[])
    }
    use super::frozen::FrozenStaticContext;
    use super::memory::load_memory;
    use super::skills::{dedupe_reference_paths, dedupe_skill_ids};
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_memory_empty_when_missing() {
        let tmp = TempDir::new().expect("tmp");
        assert!(load_memory(tmp.path(), 4096).is_empty());
    }

    #[test]
    fn load_memory_reads_index_and_body() {
        let tmp = TempDir::new().expect("tmp");
        let mem = tmp.path().join("memory");
        std::fs::create_dir_all(&mem).expect("dir");
        std::fs::write(mem.join("MEMORY.md"), "[hero]\n").expect("w");
        std::fs::write(mem.join("hero.md"), "bio").expect("w");
        let out = load_memory(tmp.path(), 4096);
        assert!(out.contains("hero"));
        assert!(out.contains("bio"));
    }

    #[test]
    fn filter_loadable_skill_ids_drops_deleted_skills() {
        let tmp = TempDir::new().expect("tmp");
        let skill_dir = tmp.path().join("skills").join("kept");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: kept\ndescription: d\n---\n# ok\n",
        )
        .expect("w");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let ids = filter_loadable_skill_ids(
            tmp.path(),
            &agent_skills,
            &["kept".into(), "removed".into(), "kept".into()],
        );
        assert_eq!(ids, vec!["kept"]);
    }

    #[test]
    fn format_invoked_skill_bodies_skips_missing_without_fallback() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["gone".into()]);
        assert!(bodies.is_empty());
    }

    #[test]
    fn format_invoked_skill_bodies_from_project_skills() {
        let tmp = TempDir::new().expect("tmp");
        let skill_dir = tmp.path().join("skills").join("xianxia");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        let marker = "仙侠规则";
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: xianxia\ndescription: d\n---\n# {marker}\n"),
        )
        .expect("w");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["xianxia".into()]);
        assert!(bodies.contains(marker));
    }

    #[test]
    fn dedupe_skill_ids_preserves_first_seen_order() {
        let ids = dedupe_skill_ids(&["a".into(), "b".into(), "a".into(), "c".into(), "b".into()]);
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn format_invoked_skill_bodies_dedupes_ids() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        let skill_dir = agent_skills.join("apocalypse");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: apocalypse\ndescription: d\n---\n# apocalypse body\n",
        )
        .expect("w");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["apocalypse".into()]);
        assert!(bodies.contains("apocalypse body"));
    }

    #[test]
    fn build_dynamic_context_includes_progress() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("chapters")).expect("dir");
        let db = Database::open(tmp.path().join("t.db")).expect("db");
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .expect("s");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let ctx = build_dynamic_context(tmp.path(), &sid, &db, "agents", &agent_skills);
        assert!(ctx.progress.contains("已完成章节文件"));
    }

    #[test]
    fn refresh_system_dynamic_context_reloads_skill_summaries_from_disk() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("chapters")).expect("dir");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let db = Database::open(tmp.path().join("t.db")).expect("db");
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .expect("s");
        let frozen = FrozenStaticContext {
            agents_md: "agents".into(),
            workspace_path: tmp
                .path()
                .canonicalize()
                .unwrap_or_else(|_| tmp.path().to_path_buf())
                .display()
                .to_string(),
        };

        let before = refresh_system_dynamic_context(tmp.path(), &sid, &db, &agent_skills, &frozen);
        assert!(before.skill_summaries.is_empty());

        write_skill(&tmp, "mid-session", "# body");

        let after = refresh_system_dynamic_context(tmp.path(), &sid, &db, &agent_skills, &frozen);
        assert_eq!(after.skill_summaries.len(), 1);
        assert_eq!(after.skill_summaries[0].0, "mid-session");
    }

    fn write_reference(tmp: &TempDir, skill_id: &str, name: &str, body: &str) {
        let dir = tmp.path().join("skills").join(skill_id).join("references");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(name), body).expect("w");
    }

    fn write_skill(tmp: &TempDir, skill_id: &str, body: &str) {
        let dir = tmp.path().join("skills").join(skill_id);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill_id}\ndescription: d\n---\n{body}\n"),
        )
        .expect("w");
    }

    #[test]
    fn parse_skill_reference_path_project_relative() {
        let tmp = TempDir::new().expect("tmp");
        write_reference(&tmp, "apocalypse", "zombie.md", "# zombie");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let parsed = parse_skill_reference_path(
            tmp.path(),
            &agent_skills,
            "skills/apocalypse/references/zombie.md",
        );
        assert_eq!(
            parsed,
            Some((
                "apocalypse".into(),
                "apocalypse/references/zombie.md".into()
            ))
        );
    }

    #[test]
    fn parse_skill_reference_path_agent_bundle() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        let dir = agent_skills.join("romance").join("references");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("harem.md"), "# h").expect("w");
        let abs = dir.join("harem.md");
        let parsed = parse_skill_reference_path(tmp.path(), &agent_skills, abs.to_str().unwrap());
        assert_eq!(
            parsed,
            Some(("romance".into(), "romance/references/harem.md".into()))
        );
    }

    #[test]
    fn parse_skill_reference_path_rejects_non_reference() {
        let tmp = TempDir::new().expect("tmp");
        write_skill(&tmp, "apocalypse", "# skill");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        assert!(parse_skill_reference_path(
            tmp.path(),
            &agent_skills,
            "skills/apocalypse/SKILL.md"
        )
        .is_none());
    }

    #[test]
    fn format_activated_skill_block_groups_references_under_skill() {
        let tmp = TempDir::new().expect("tmp");
        write_skill(&tmp, "apocalypse", "# apocalypse body");
        write_reference(&tmp, "apocalypse", "zombie.md", "# zombie ref");
        write_skill(&tmp, "romance", "# romance body");
        write_reference(&tmp, "romance", "harem.md", "# harem ref");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let refs = vec![
            "apocalypse/references/zombie.md".into(),
            "romance/references/harem.md".into(),
        ];
        let block = format_activated_skill_block(
            tmp.path(),
            &agent_skills,
            &["apocalypse".into(), "romance".into()],
            &refs,
        );
        let romance_pos = block.find("### romance\n").expect("skill2");
        let harem_pos = block
            .find("### romance/references/harem.md\n")
            .expect("ref2");
        let apocalypse_pos = block.find("### apocalypse\n").expect("skill");
        let zombie_pos = block
            .find("### apocalypse/references/zombie.md\n")
            .expect("ref");
        assert!(romance_pos < harem_pos);
        assert!(harem_pos < apocalypse_pos);
        assert!(apocalypse_pos < zombie_pos);
        assert!(block.contains("zombie ref"));
    }

    #[test]
    fn filter_loadable_reference_paths_requires_invoked_parent() {
        let tmp = TempDir::new().expect("tmp");
        write_reference(&tmp, "apocalypse", "zombie.md", "# z");
        write_reference(&tmp, "romance", "harem.md", "# h");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let paths = vec![
            "apocalypse/references/zombie.md".into(),
            "romance/references/harem.md".into(),
        ];
        let filtered = filter_loadable_reference_paths(
            tmp.path(),
            &agent_skills,
            &paths,
            &["apocalypse".into()],
        );
        assert_eq!(filtered, vec!["apocalypse/references/zombie.md"]);
    }

    #[test]
    fn dedupe_reference_paths_preserves_order() {
        let paths = dedupe_reference_paths(&[
            "a/references/x.md".into(),
            "b/references/y.md".into(),
            "a/references/x.md".into(),
        ]);
        assert_eq!(paths, vec!["a/references/x.md", "b/references/y.md"]);
    }
}
