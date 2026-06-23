//! Live connectivity checks against DeepSeek endpoints from `config.toml`.
//! Run: `DEEPSEEK_API_KEY=sk-... cargo test -p novel-deepseek -- --ignored --nocapture live_endpoints`

#![allow(clippy::unwrap_used)]
use novel_deepseek::{
    verify_endpoints, verify_web_search_endpoint, ChatClient, ChatStreamConfig, LlmChatMessage,
    StreamEvent, StreamOutcome,
};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::time::Duration;

#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and network"]
async fn live_endpoints_from_config() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY");
    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    verify_endpoints(&api_key, &model)
        .await
        .expect("chat + web_search endpoints should be reachable");
}

#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and network"]
async fn live_web_search_endpoint_from_config() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY");
    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    verify_web_search_endpoint(&api_key, &model)
        .await
        .expect("web_search endpoint should return web_search_tool_result");
}

/// Test that interrupting a streaming request produces valid three-class
/// token data via `drain_usage_background`.
///
/// This simulates a user interrupt:
/// 1. Start a streaming request
/// 2. Cancel via AbortController
/// 3. Await drain → assert cache_hit/cache_miss/completion all valid
#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and network"]
async fn drain_after_interrupt_returns_three_class_tokens() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY");
    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    let mut client = ChatClient::deepseek(&api_key, &model, "https://api.deepseek.com/v1", false);

    let messages = vec![
        LlmChatMessage {
            role: "system".into(),
            content: "你是一个助手。".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
        LlmChatMessage {
            role: "user".into(),
            content: "请用100个字介绍北京的旅游景点。".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        },
    ];

    // Cancel immediately via Arc<AtomicBool>
    let cancel = Arc::new(AtomicBool::new(true));

    let result = client
        .create_stream(
            &messages,
            &[],
            ChatStreamConfig {
                max_tokens: 512,
                options: novel_deepseek::ChatRequestOptions::default(),
                cancel: Some(Arc::clone(&cancel)),
            },
            |_: StreamEvent| {},
            None::<fn(novel_deepseek::LlmToolCall)>,
        )
        .await
        .expect("create_stream should not error");

    // Should be cancelled immediately
    let rx = match result {
        StreamOutcome::Cancelled {
            partial,
            background_usage,
        } => {
            assert!(
                partial.content.is_none() || partial.content.as_deref() == Some(""),
                "cancelled before first chunk → content empty"
            );
            assert!(
                partial.usage.is_none(),
                "cancelled before first chunk → no SSE usage yet (drain provides it)"
            );
            background_usage
        }
        StreamOutcome::Complete(_) => {
            panic!("expected Cancelled, got Complete (message too short?)");
        }
    };

    // Await drain — should return valid three-class token data
    let usage = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("drain request timed out")
        .expect("drain oneshot was dropped")
        .expect("drain returned None (token data missing)");

    let total_prompt = usage.cache_hit_tokens + usage.cache_miss_tokens;
    assert!(
        total_prompt > 0,
        "prompt tokens should be > 0, got {total_prompt}"
    );
    assert!(
        usage.completion_tokens > 0,
        "completion tokens should be > 0 (drain generates 1 at min), got {}",
        usage.completion_tokens
    );
    // Cache hit is optional (first request may not hit cache), but with
    // identical drain messages prefix cache should hit.
    assert!(
        usage.cache_hit_tokens >= 0 && usage.cache_miss_tokens >= 0,
        "hit={} miss={} — both should be non-negative",
        usage.cache_hit_tokens,
        usage.cache_miss_tokens
    );

    println!(
        "Drain after interrupt: hit={}, miss={}, completion={}, total_prompt={}",
        usage.cache_hit_tokens, usage.cache_miss_tokens, usage.completion_tokens, total_prompt,
    );
}

/// Same as above but with thinking enabled to verify drain includes
/// `thinking` config and still returns valid tokens.
#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and network"]
async fn drain_with_thinking_returns_three_class_tokens() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY");
    let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    let mut client = ChatClient::deepseek(&api_key, &model, "https://api.deepseek.com/v1", true);

    let messages = vec![LlmChatMessage {
        role: "user".into(),
        content: "用50字介绍西湖。".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    }];

    let cancel = Arc::new(AtomicBool::new(true));
    let result = client
        .create_stream(
            &messages,
            &[],
            ChatStreamConfig {
                max_tokens: 256,
                options: novel_deepseek::ChatRequestOptions::default(),
                cancel: Some(Arc::clone(&cancel)),
            },
            |_: StreamEvent| {},
            None::<fn(novel_deepseek::LlmToolCall)>,
        )
        .await
        .expect("create_stream should not error");

    let rx = match result {
        StreamOutcome::Cancelled {
            background_usage, ..
        } => background_usage,
        StreamOutcome::Complete(_) => panic!("expected Cancelled"),
    };

    let usage = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("drain timeout")
        .expect("drain dropped")
        .expect("drain returned None");

    assert!(usage.cache_hit_tokens + usage.cache_miss_tokens > 0);
    assert!(usage.completion_tokens > 0);

    println!(
        "Drain with thinking: hit={}, miss={}, completion={}",
        usage.cache_hit_tokens, usage.cache_miss_tokens, usage.completion_tokens,
    );
}
