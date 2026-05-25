//! PostToolUse hook matchers (settings opt-in). Matching rules enqueue **LogIntegrityChecker subagent**
//! tasks into `pending_hook_tasks` → `drain_pending_hooks`. "Hook" is the PostToolUse trigger path,
//! not a separate agent type from Subagent/fork.

use novel_config::{HookConfig, HookMatcher};
use novel_tools::ToolRegistry;
use serde_json::Value;

/// Default hooks: empty (LLM decides when to Fork LogIntegrityChecker). Users may opt in via settings.json.
pub fn default_hook_config() -> HookConfig {
    HookConfig {
        post_tool_use: vec![],
    }
}

/// Run PostToolUse hooks matching the tool name and optional file path.
pub fn run_post_tool_use_hooks(
    hooks: &HookConfig,
    tool_name: &str,
    tool_input: Option<&Value>,
    tool_output: &str,
) -> Vec<String> {
    let mut prompts = Vec::new();
    for HookMatcher {
        matcher,
        hooks: rules,
    } in &hooks.post_tool_use
    {
        if matcher_matches(matcher, tool_name, tool_input) {
            for rule in rules {
                if rule.hook_type == "prompt" || rule.hook_type == "agent" {
                    prompts.push(format!(
                        "{}\n\nTool: {tool_name}\nOutput preview: {}",
                        rule.prompt,
                        truncate(tool_output, 500)
                    ));
                }
            }
        }
    }
    prompts
}

/// Build sub-agent task prompt when PostToolUse hooks match.
/// Matching is driven by `hooks.post_tool_use[].matcher` — no tool-name hardcoding.
pub fn log_integrity_checker_task(
    hooks: &HookConfig,
    tool_name: &str,
    tool_input: Option<&Value>,
    tool_output: &str,
) -> Option<String> {
    let prompts = run_post_tool_use_hooks(hooks, tool_name, tool_input, tool_output);
    if prompts.is_empty() {
        return None;
    }
    Some(format!(
        "LogIntegrityChecker 扫描任务：\n{}\n\n请检查上述工具输出，列出演变日志遗漏并给出建议 append 行（只读报告，禁止 Write/Edit）。",
        prompts.join("\n\n---\n\n")
    ))
}

pub fn tool_schemas_for_agent(
    registry: &ToolRegistry,
    allowed: &[String],
) -> Vec<(String, String, serde_json::Value)> {
    allowed
        .iter()
        .filter_map(|name| {
            registry.get(name).map(|t| {
                let mut desc = t.description().to_string();
                let hint = t.usage_hint();
                if !hint.is_empty() {
                    desc.push_str(" — ");
                    desc.push_str(hint);
                }
                (t.name().to_string(), desc, t.input_schema())
            })
        })
        .collect()
}

fn tool_target_path(input: Option<&Value>) -> Option<String> {
    let v = input?;
    v.get("file_path")
        .or_else(|| v.get("path"))
        .and_then(|p| p.as_str())
        .map(String::from)
}

fn matcher_matches(matcher: &str, tool_name: &str, tool_input: Option<&Value>) -> bool {
    if matcher == "*" || matcher.is_empty() {
        return true;
    }
    if matcher.starts_with("Write(chapters/**)") || matcher.contains("Write(chapters/**)") {
        if tool_name != "Write" && tool_name != "Edit" {
            return false;
        }
        return tool_target_path(tool_input)
            .is_some_and(|p| p.replace('\\', "/").contains("chapters/"));
    }
    if matcher.starts_with("Write|Edit") {
        return tool_name == "Write" || tool_name == "Edit";
    }
    matcher == tool_name
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_hook_config_is_empty() {
        let hooks = default_hook_config();
        assert!(hooks.post_tool_use.is_empty());
    }

    #[test]
    fn log_integrity_checker_skips_when_no_hooks() {
        let hooks = default_hook_config();
        let input = serde_json::json!({"file_path": "chapters/chapter-031.md"});
        assert!(log_integrity_checker_task(&hooks, "Write", Some(&input), "w").is_none());
    }

    #[test]
    fn tool_schemas_filtered() {
        let reg = novel_tools::default_registry(PathBuf::from("."));
        let schemas = tool_schemas_for_agent(&reg, &["Read".into(), "NoSuch".into()]);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].0, "Read");
    }
}
