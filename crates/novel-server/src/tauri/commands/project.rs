use crate::tauri::engine_loop::WorkSummary;
use crate::tauri::session_api;
use crate::tauri::state::CommandContext;

use super::session::switch_project_and_create_session;

pub async fn init_novel_project(ctx: &CommandContext) -> Result<(), String> {
    let (work, templates) = {
        let cfg = ctx.config.read().await;
        (cfg.active_project.clone(), cfg.templates_dir())
    };
    novel_knowledge::init_project_scaffold(&work, templates.as_path()).map_err(|e| e.to_string())
}

pub fn list_works(works_dir: &std::path::Path) -> Result<Vec<WorkSummary>, String> {
    if !works_dir.is_dir() {
        return Ok(Vec::new());
    }
    std::fs::read_dir(works_dir).map_err(|e| e.to_string())?;
    let mut works = session_api::list_work_dirs(works_dir)
        .into_iter()
        .map(|name| {
            let path = works_dir.join(&name);
            let initialized = path.join("AGENTS.md").is_file() || path.join("knowledge").is_dir();
            WorkSummary {
                path: path.to_string_lossy().into_owned(),
                name,
                initialized,
            }
        })
        .collect::<Vec<_>>();
    works.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(works)
}

pub async fn list_works_cmd(ctx: &CommandContext) -> Result<Vec<WorkSummary>, String> {
    let cfg = ctx.config.read().await;
    list_works(&cfg.works_dir)
}

pub async fn create_work(ctx: &CommandContext, name: String) -> Result<String, String> {
    use novel_config::work_path;
    let (work, templates) = {
        let cfg = ctx.config.read().await;
        (
            work_path(&cfg.agent_root, &name).map_err(|e| e.to_string())?,
            cfg.templates_dir(),
        )
    };
    if !work.exists() {
        novel_knowledge::init_project_scaffold(&work, templates.as_path())
            .map_err(|e| e.to_string())?;
    }
    switch_project_and_create_session(ctx, work).await
}

pub async fn open_work(ctx: &CommandContext, name: String) -> Result<String, String> {
    let work = {
        let cfg = ctx.config.read().await;
        novel_config::work_path(&cfg.agent_root, &name).map_err(|e| e.to_string())?
    };
    if !work.exists() {
        return Err(format!("work not found: {name}"));
    }
    switch_project_and_create_session(ctx, work).await
}

pub async fn list_project_files(
    ctx: &CommandContext,
) -> Result<Vec<novel_knowledge::ProjectFileEntry>, String> {
    let cfg = ctx.config.read().await;
    novel_knowledge::list_project_files(&cfg.active_project).map_err(|e| e.to_string())
}

pub async fn read_project_file(ctx: &CommandContext, path: String) -> Result<String, String> {
    let cfg = ctx.config.read().await;
    novel_knowledge::read_project_file(&cfg.active_project, &path).map_err(|e| e.to_string())
}
