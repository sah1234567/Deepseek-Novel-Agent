use crate::ChatMessage;
use serde_json::Value;

/// Serialize a ChatMessage to JSON for SQLite.  display_content: ""
/// hides the message from the frontend while keeping content for the LLM.
pub fn chat_to_json(msg: &ChatMessage) -> Value {
    let mut obj = serde_json::json!({ "content": msg.content });
    if let Some(id) = &msg.tool_call_id {
        obj["tool_call_id"] = Value::String(id.clone());
    }
    if let Some(tcs) = &msg.tool_calls {
        obj["tool_calls"] = serde_json::to_value(tcs).unwrap_or_else(|e| {
            tracing::warn!(%e, "failed to serialize tool_calls to JSON");
            Value::Null
        });
    }
    if let Some(rc) = &msg.reasoning_content {
        if !rc.is_empty() {
            obj["reasoning_content"] = Value::String(rc.clone());
        }
    }
    if let Some(dc) = &msg.display_content {
        obj["display_content"] = Value::String(dc.clone());
    }
    obj
}

/// Persist JSON for a chat row. `display_content` is UI-only metadata (never sent to the LLM).
pub fn chat_to_json_for_persist(msg: &ChatMessage, display_content: Option<&str>) -> Value {
    let mut obj = chat_to_json(msg);
    if let Some(display) = display_content {
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "display_content".to_string(),
                Value::String(display.to_string()),
            );
        }
    }
    if let Some(dc) = &msg.display_content {
        obj["display_content"] = Value::String(dc.clone());
    }
    obj
}

/// UI display text from stored `content_json`. LLM paths must use `content` only via `stored_to_chat`.
pub fn stored_message_display_text(content_json: &Value) -> String {
    if let Some(display) = content_json.get("display_content").and_then(|v| v.as_str()) {
        return display.to_string();
    }
    content_json
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}
