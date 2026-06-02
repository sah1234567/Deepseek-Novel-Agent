use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct SearchSource {
    title: String,
    url: String,
    key_points: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WebSearchOutput {
    summary: String,
    sources: Vec<SearchSource>,
    cached_path: String,
}

pub struct WebSearchTool;

/// Cached search results under `.websearch/` (Agent runtime cache, not novel canon).
const CACHE_DIR: &str = ".websearch";

pub(crate) fn compose_search_query(query: &str, aspect: &str, genre: &str) -> String {
    if genre.is_empty() {
        format!("{query} {aspect}")
    } else {
        format!("{genre} {query} {aspect}")
    }
}

async fn perform_search(ctx: &ToolContext, query: &str) -> Result<Vec<SearchSource>, ToolError> {
    let api_key = ctx.resolve_deepseek_api_key().ok_or_else(|| {
        ToolError::Execution(
            "联网搜索未配置 API Key：请在设置中填写 DeepSeek API Key，或设置环境变量 DEEPSEEK_API_KEY"
                .into(),
        )
    })?;

    let results = novel_deepseek::ChatClient::web_search(&api_key, query, 8)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "WebSearch API call failed");
            ToolError::Execution(format!("联网搜索失败: {e}"))
        })?;

    Ok(results
        .into_iter()
        .map(|r| SearchSource {
            title: r.title,
            url: r.url,
            key_points: if r.snippet.is_empty() {
                vec![]
            } else {
                vec![r.snippet]
            },
        })
        .collect())
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }
    fn description(&self) -> &str {
        "通用网页搜索，基于 DeepSeek web_search_20250305 服务端搜索。可用于市场调研、对标作品分析、读者反馈、桥段参考等任何需要联网搜索的场景。原始结果缓存 .websearch/（非 knowledge 正典；纳入设定请再 Write 到 plan/ 或 knowledge/）"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "搜索关键词"},
                "aspect": {
                    "type": "string",
                    "description": "搜索目的/角度，用于缓存文件命名和摘要",
                    "enum": ["research", "similar-works", "reader-feedback", "trope-reference", "fact-check", "writing-tips", "trending", "short-drama"]
                },
                "genre": {"type": "string", "description": "相关流派（可选，用于加权搜索词）"}
            },
            "required": ["query"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let query = require_str(&input, "query")?;
        let aspect = input
            .get("aspect")
            .and_then(|v| v.as_str())
            .unwrap_or("research");
        let genre = input.get("genre").and_then(|v| v.as_str()).unwrap_or("");
        let search_query = compose_search_query(&query, aspect, genre);

        let sources = perform_search(ctx, &search_query).await?;
        let summary = if sources.is_empty() {
            format!("未找到联网结果（搜索返回空），已记录搜索请求: {query} ({aspect})")
        } else {
            format!(
                "找到 {} 条来源，主题: {query} / {aspect}{}",
                sources.len(),
                if genre.is_empty() {
                    String::new()
                } else {
                    format!(" / 流派: {genre}")
                }
            )
        };

        let dir = ctx.project_root.join(CACHE_DIR);
        crate::blocking::create_dir_all(dir.clone()).await?;
        let filename = format!("search-{}-{}.md", aspect, chrono_like_slug());
        let path = dir.join(&filename);
        let mut body = format!(
            "# 网页搜索\n\n- query: {query}\n- aspect: {aspect}\n- genre: {genre}\n- search: {search_query}\n\n## 摘要\n\n{summary}\n\n## 来源\n\n"
        );
        for src in &sources {
            body.push_str(&format!("### {}\n- URL: {}\n", src.title, src.url));
            for kp in &src.key_points {
                body.push_str(&format!("- {kp}\n"));
            }
            body.push('\n');
        }
        crate::blocking::write(path.clone(), body).await?;
        let rel = path
            .strip_prefix(&ctx.project_root)
            .unwrap_or(&path)
            .display()
            .to_string();

        let output = WebSearchOutput {
            summary,
            sources,
            cached_path: rel,
        };
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&output)
                .map_err(|e| ToolError::Execution(format!("json serialize: {e}")))?,
            is_error: false,
        })
    }
}

fn chrono_like_slug() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_registry, PermissionMode, ToolCallSpec, ToolExecutor};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn compose_search_query_includes_genre() {
        let q = compose_search_query("趋势", "research", "xianxia");
        assert!(q.contains("xianxia"));
        assert!(q.contains("趋势"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn errors_when_no_api_key() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry());
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = ex
            .execute_one(
                &ToolCallSpec {
                    id: "1".into(),
                    name: "WebSearch".into(),
                    input: json!({"query": "test"}),
                },
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("API Key"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires DEEPSEEK_API_KEY and network"]
    async fn writes_cache_under_dot_websearch() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry());
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = ex
            .execute_one(
                &ToolCallSpec {
                    id: "1".into(),
                    name: "WebSearch".into(),
                    input: json!({"query": "仙侠趋势", "aspect": "trending", "genre": "xianxia"}),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("summary"));
        assert!(tmp.path().join(CACHE_DIR).exists());
    }
}
