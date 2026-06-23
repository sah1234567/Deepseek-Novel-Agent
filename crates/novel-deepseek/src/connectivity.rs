use crate::config::{chat_api_base, web_search_messages_url};
use crate::error::LlmError;
use serde_json::Value;

/// Probe chat + web-search endpoints with minimal requests (requires a valid API key).
pub async fn verify_endpoints(api_key: &str, model: &str) -> Result<(), LlmError> {
    verify_chat_endpoint(api_key, model).await?;
    verify_web_search_endpoint(api_key, model).await?;
    Ok(())
}

pub async fn verify_chat_endpoint(api_key: &str, model: &str) -> Result<(), LlmError> {
    let base = chat_api_base();
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 8,
        "stream": false,
        "thinking": { "type": "disabled" },
    });
    post_json(&url, api_key, AuthStyle::Bearer, &body, "chat")
        .await
        .map(|_| ())
}

pub async fn verify_web_search_endpoint(api_key: &str, model: &str) -> Result<(), LlmError> {
    let url = web_search_messages_url();
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 256,
        "system": "You are an assistant for performing a web search tool use",
        "messages": [{
            "role": "user",
            "content": "Perform a web search for the query: DeepSeek API documentation"
        }],
        "tools": [{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 1
        }],
        "stream": false,
    });
    let text = post_json(
        &url,
        api_key,
        AuthStyle::AnthropicApiKey,
        &body,
        "web_search",
    )
    .await?;
    let json: Value = serde_json::from_str(&text)
        .map_err(|e| LlmError::Api(format!("web_search response not JSON: {e}")))?;
    let has_search_result = json
        .get("content")
        .and_then(|c| c.as_array())
        .is_some_and(|blocks| {
            blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("web_search_tool_result"))
        });
    if !has_search_result {
        return Err(LlmError::Api(
            "web_search endpoint responded but no web_search_tool_result block found".into(),
        ));
    }
    Ok(())
}

enum AuthStyle {
    Bearer,
    AnthropicApiKey,
}

async fn post_json(
    url: &str,
    api_key: &str,
    auth: AuthStyle,
    body: &Value,
    label: &str,
) -> Result<String, LlmError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| LlmError::Api(e.to_string()))?;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(body);
    req = match auth {
        AuthStyle::Bearer => req.header("Authorization", format!("Bearer {api_key}")),
        AuthStyle::AnthropicApiKey => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    };
    let resp = req.send().await.map_err(|e| LlmError::Api(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(LlmError::Api(format!(
            "{label} endpoint HTTP {status}: {text}"
        )));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn post_json_returns_200_on_success() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
            .mount(&mock_server)
            .await;

        let url = format!("{}/chat/completions", mock_server.uri());
        let body = serde_json::json!({ "model": "test" });
        let text = post_json(&url, "test-key", AuthStyle::Bearer, &body, "chat")
            .await
            .expect("post_json should succeed");
        assert_eq!(text, r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn verify_web_search_endpoint_accepts_tool_result() {
        let mock_server = MockServer::start().await;
        std::env::set_var(
            "DEEPSEEK_WEB_SEARCH_MESSAGES_URL",
            format!("{}/v1/messages", mock_server.uri()),
        );
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"content":[{"type":"web_search_tool_result","tool_use_id":"t1"}]}"#,
            ))
            .mount(&mock_server)
            .await;
        verify_web_search_endpoint("test-key", "deepseek-chat")
            .await
            .expect("web search verify");
        std::env::remove_var("DEEPSEEK_WEB_SEARCH_MESSAGES_URL");
    }
}
