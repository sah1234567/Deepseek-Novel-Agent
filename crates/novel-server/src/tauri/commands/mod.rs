mod engine_ipc;
mod fork;
mod project;
mod session;
mod settings;
mod turn;

#[cfg(test)]
mod tests;

pub use fork::*;
pub use project::*;
pub use session::*;
pub use settings::*;
pub use turn::*;

use super::state::CommandContext;

pub(crate) async fn open_db(ctx: &CommandContext) -> Result<novel_state::Database, String> {
    let cfg = ctx.config.read().await;
    novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())
}
