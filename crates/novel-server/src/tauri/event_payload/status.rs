use novel_core::Event;

use super::serialize_payload;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InterruptibleStatusPayload {
    has_interruptible_tool_in_progress: bool,
}

pub(crate) fn interruptible_payload(event: &Event) -> Option<(String, serde_json::Value)> {
    let Event::InterruptibleStatusChanged {
        has_interruptible_tool_in_progress,
    } = event
    else {
        return None;
    };
    serialize_payload(
        "interruptible-status-changed",
        &InterruptibleStatusPayload {
            has_interruptible_tool_in_progress: *has_interruptible_tool_in_progress,
        },
    )
    .map(|payload| ("interruptible-status-changed".into(), payload))
}
