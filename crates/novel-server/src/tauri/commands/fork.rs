use crate::tauri::state::CommandContext;
use novel_core::ForkStreamSubscriptions;

pub fn clear_fork_stream_subscriptions(subs: &ForkStreamSubscriptions) {
    if let Ok(mut guard) = subs.write() {
        guard.clear();
    }
}

pub fn subscribe_fork_stream(ctx: &CommandContext, run_id: String) -> Result<(), String> {
    ctx.fork_stream_subs
        .write()
        .map_err(|_| "fork stream subscriptions lock poisoned".to_string())?
        .insert(run_id);
    Ok(())
}

pub fn unsubscribe_fork_stream(ctx: &CommandContext, run_id: String) -> Result<(), String> {
    ctx.fork_stream_subs
        .write()
        .map_err(|_| "fork stream subscriptions lock poisoned".to_string())?
        .remove(&run_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_core::{is_fork_stream_subscribed, new_fork_stream_subscriptions};

    #[test]
    fn clear_removes_all_subscriptions() {
        let subs = new_fork_stream_subscriptions();
        subs.write().unwrap().insert("fr-1".into());
        subs.write().unwrap().insert("fr-2".into());
        clear_fork_stream_subscriptions(&subs);
        assert!(!is_fork_stream_subscribed(&subs, "fr-1"));
        assert!(!is_fork_stream_subscribed(&subs, "fr-2"));
    }
}
