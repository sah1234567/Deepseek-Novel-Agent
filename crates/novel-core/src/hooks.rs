//! PostToolUse hook matchers (settings opt-in). Matching rules enqueue **KnowledgeAuditor** work on
//! `EngineShared.subagent_queue` (`parent_tool_call_id: None`); drained by `drain_subagent_jobs`.

use novel_config::{HookConfig, HookMatcher};
use novel_knowledge::truncate_chars;
use novel_tools::{normalize_rel_path, optional_file_path, ToolRegistry};
use serde_json::Value;

/// Default hooks: empty (LLM decides when to Fork KnowledgeAuditor). Users may opt in via settings.json.
pub fn default_hook_config() -> HookConfig {
    HookConfig {
        post_tool_use: vec![],
    }
}

/// Run PostToolUse hooks matching the tool name and optional file path.
pub(crate) fn run_post_tool_use_hooks(
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
                        truncate_chars(tool_output, 500)
                    ));
                }
            }
        }
    }
    prompts
}

/// Build sub-agent task prompt when PostToolUse hooks match.
pub fn knowledge_auditor_hook_task(
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
        "KnowledgeAuditor 轻量扫描任务：\n{}\n\n请检查上述工具输出，列出演变日志遗漏并给出建议 append 行（只读报告，禁止 Write/Edit）。",
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

/// Full registry tool list (sorted). Test helper / legacy full-schema dump.
#[cfg(test)]
pub fn main_tool_names(registry: &ToolRegistry) -> Vec<String> {
    registry.names()
}

/// All registered tool schemas (unfiltered). Prefer [`tool_schemas_for_visibility`] for the main agent.
#[cfg(test)]
pub fn main_tool_schemas(registry: &ToolRegistry) -> Vec<(String, String, serde_json::Value)> {
    tool_schemas_for_agent(registry, &main_tool_names(registry))
}

/// Main-agent tool visibility by plan / focus state (plan 2.1 layering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVisibility {
    /// No formal plan (missing file or empty skeleton): build with PlanBuilder.
    Interview,
    /// Formal plan, no focused node: graph lifecycle + scheduling (no Write/Edit).
    Orchestrator,
    /// Focused on a running node: content tools + gate submit (no PlanBuilder / Advance / Fork).
    NodeExecution,
}

/// Interview / 建图：PlanBuilder + 只读探测；无 Write/Edit/GraphAdvance/Fork。
const INTERVIEW_TOOLS: &[&str] = &[
    "PlanBuilder",
    "GraphCommitPlan",
    "GraphQuery",
    "Read",
    "Tail",
    "Grep",
    "Glob",
    "InvokeSkill",
    "AskUserQuestion",
    "TodoWrite",
    "WebSearch",
    "CharacterSearch",
    "PlotGraph",
    "PlotGrid",
    "ForeshadowTracker",
    "Corkboard",
    "Stats",
    "AuditStatusQuery",
    "TrackingQuery",
    "RelationQuery",
];

/// 编排器：图生命周期 + Fork + 只读知识；无 Write/Edit/Bash。
const ORCHESTRATOR_TOOLS: &[&str] = &[
    "PlanBuilder",
    "GraphQuery",
    "GraphAdvance",
    "GraphSubmitForApproval",
    "GraphMarkVerified",
    "GraphReopen",
    "GraphCommitPlan",
    "ForkSubAgent",
    "InvokeSkill",
    "Read",
    "Tail",
    "Grep",
    "Glob",
    "AskUserQuestion",
    "TodoWrite",
    "CharacterSearch",
    "PlotGraph",
    "ForeshadowTracker",
    "TrackingQuery",
    "RelationQuery",
    "Corkboard",
    "Stats",
    "AuditStatusQuery",
];

/// 节点执行：写内容 + 全知识工具；保留出门闸工具；无 PlanBuilder / GraphAdvance / Fork。
const NODE_EXECUTION_TOOLS: &[&str] = &[
    "Write",
    "Edit",
    "Read",
    "Tail",
    "Grep",
    "Glob",
    "Bash",
    "WebSearch",
    "InvokeSkill",
    "TodoWrite",
    "AskUserQuestion",
    "CharacterSearch",
    "PlotGraph",
    "PlotGrid",
    "ForeshadowTracker",
    "Stats",
    "Corkboard",
    "CharacterRotate",
    "ImpactAnalysis",
    "KnowledgeDerive",
    "AuditStatusQuery",
    "AuditStatusUpdate",
    "TrackingQuery",
    "RelationQuery",
    // Gate tools while focused (submit / verify / reopen current node).
    "GraphQuery",
    "GraphSubmitForApproval",
    "GraphMarkVerified",
    "GraphReopen",
];

/// Resolve visibility from interaction mode + plan presence + focus.
///
/// - **Orchestrate**: Interview (no/empty plan) or Orchestrator — never NodeExecution,
///   even if a stale focus remains.
/// - **Work**: NodeExecution only when a node is focused; otherwise degrades to the
///   orchestrate plan layer (defensive; UI/IPC should reject Work without focus).
pub fn resolve_tool_visibility(
    project_root: &std::path::Path,
    interaction: crate::InteractionMode,
) -> ToolVisibility {
    let plan_layer = resolve_plan_tool_layer(project_root);
    match interaction {
        crate::InteractionMode::Orchestrate => plan_layer,
        crate::InteractionMode::Work => {
            if focused_node_id(project_root).is_some() {
                ToolVisibility::NodeExecution
            } else {
                plan_layer
            }
        }
    }
}

fn focused_node_id(project_root: &std::path::Path) -> Option<String> {
    novel_graph::GraphTracker::load(project_root)
        .ok()
        .flatten()
        .and_then(|t| t.state.focused_node_id)
}

fn resolve_plan_tool_layer(project_root: &std::path::Path) -> ToolVisibility {
    if !novel_graph::plan_exists(project_root) {
        return ToolVisibility::Interview;
    }
    let Some(tracker) = novel_graph::GraphTracker::load(project_root).ok().flatten() else {
        return ToolVisibility::Interview;
    };
    if tracker.plan.nodes.is_empty() {
        ToolVisibility::Interview
    } else {
        ToolVisibility::Orchestrator
    }
}

/// Schemas for the main agent under the given visibility mode.
pub fn tool_schemas_for_visibility(
    registry: &ToolRegistry,
    visibility: ToolVisibility,
) -> Vec<(String, String, serde_json::Value)> {
    let allow: &[&str] = match visibility {
        ToolVisibility::Interview => INTERVIEW_TOOLS,
        ToolVisibility::Orchestrator => ORCHESTRATOR_TOOLS,
        ToolVisibility::NodeExecution => NODE_EXECUTION_TOOLS,
    };
    let names: Vec<String> = allow.iter().map(|s| (*s).to_string()).collect();
    tool_schemas_for_agent(registry, &names)
}

fn matcher_matches(matcher: &str, tool_name: &str, tool_input: Option<&Value>) -> bool {
    if matcher == "*" || matcher.is_empty() {
        return true;
    }
    if matcher.starts_with("Write(chapters/**)") || matcher.contains("Write(chapters/**)") {
        if tool_name != "Write" && tool_name != "Edit" {
            return false;
        }
        let Some(v) = tool_input else {
            return false;
        };
        return optional_file_path(v).is_some_and(|p| normalize_rel_path(&p).contains("chapters/"));
    }
    if matcher.starts_with("Write|Edit") {
        return tool_name == "Write" || tool_name == "Edit";
    }
    matcher == tool_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hook_config_is_empty() {
        let hooks = default_hook_config();
        assert!(hooks.post_tool_use.is_empty());
    }

    use rstest::rstest;

    #[rstest]
    #[case("*", "Read", None, true)]
    #[case("", "Write", None, true)]
    #[case("Read", "Read", None, true)]
    #[case("Read", "Write", None, false)]
    #[case("Write|Edit", "Edit", None, true)]
    #[case("Write|Edit", "Grep", None, false)]
    fn matcher_matches_cases(
        #[case] matcher: &str,
        #[case] tool: &str,
        #[case] input: Option<serde_json::Value>,
        #[case] expected: bool,
    ) {
        assert_eq!(matcher_matches(matcher, tool, input.as_ref()), expected);
    }

    #[test]
    fn knowledge_auditor_hook_skips_when_no_hooks() {
        let hooks = default_hook_config();
        let input = serde_json::json!({"file_path": "chapters/chapter-031.md"});
        assert!(knowledge_auditor_hook_task(&hooks, "Write", Some(&input), "w").is_none());
    }

    #[test]
    fn post_tool_use_truncates_utf8_output_preview() {
        let hooks = HookConfig {
            post_tool_use: vec![HookMatcher {
                matcher: "*".into(),
                hooks: vec![novel_config::HookRule {
                    hook_type: "prompt".into(),
                    prompt: "check".into(),
                    timeout: 60,
                }],
            }],
        };
        let long = "测".repeat(600);
        let prompts = run_post_tool_use_hooks(&hooks, "Read", None, &long);
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains('…'));
    }

    #[test]
    fn main_tool_schemas_match_registry_names() {
        let reg = novel_tools::default_registry();
        let names = main_tool_names(&reg);
        let schemas = main_tool_schemas(&reg);
        assert_eq!(schemas.len(), names.len());
        for (i, (n, _, _)) in schemas.iter().enumerate() {
            assert_eq!(n, &names[i]);
        }
    }

    #[test]
    fn tool_schemas_filtered() {
        let reg = novel_tools::default_registry();
        let schemas = tool_schemas_for_agent(&reg, &["Read".into(), "NoSuch".into()]);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].0, "Read");
    }

    #[test]
    fn visibility_interview_excludes_write_and_advance() {
        let reg = novel_tools::default_registry();
        let schemas = tool_schemas_for_visibility(&reg, ToolVisibility::Interview);
        let names: Vec<_> = schemas.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"PlanBuilder"));
        assert!(!names.contains(&"Write"));
        assert!(!names.contains(&"Edit"));
        assert!(!names.contains(&"GraphAdvance"));
        assert!(!names.contains(&"ForkSubAgent"));
    }

    #[test]
    fn visibility_orchestrator_excludes_write_includes_graph() {
        let reg = novel_tools::default_registry();
        let schemas = tool_schemas_for_visibility(&reg, ToolVisibility::Orchestrator);
        let names: Vec<_> = schemas.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"GraphAdvance"));
        assert!(names.contains(&"PlanBuilder"));
        assert!(names.contains(&"ForkSubAgent"));
        assert!(!names.contains(&"Write"));
        assert!(!names.contains(&"Edit"));
        assert!(!names.contains(&"Bash"));
        assert!(!names.contains(&"GraphApplyTemplate"));
    }

    #[test]
    fn visibility_node_includes_write_excludes_plan_builder() {
        let reg = novel_tools::default_registry();
        let schemas = tool_schemas_for_visibility(&reg, ToolVisibility::NodeExecution);
        let names: Vec<_> = schemas.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"Write"));
        assert!(names.contains(&"Edit"));
        assert!(names.contains(&"GraphSubmitForApproval"));
        assert!(!names.contains(&"PlanBuilder"));
        assert!(!names.contains(&"GraphAdvance"));
        assert!(!names.contains(&"ForkSubAgent"));
    }

    #[test]
    fn resolve_visibility_empty_root_is_interview() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        assert_eq!(
            resolve_tool_visibility(tmp.path(), crate::InteractionMode::Orchestrate),
            ToolVisibility::Interview
        );
        assert_eq!(
            resolve_tool_visibility(tmp.path(), crate::InteractionMode::Work),
            ToolVisibility::Interview
        );
    }

    #[test]
    fn resolve_orchestrate_ignores_stale_focus() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let plan = novel_graph::PlanGraph {
            version: "1".into(),
            nodes: vec![novel_graph::PlanNode {
                id: "a".into(),
                title: "A".into(),
                spec: Some("s".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        novel_graph::save_plan(tmp.path(), &plan).unwrap();
        let mut t = novel_graph::GraphTracker::new(plan);
        t.set_focus(Some("a".into())).unwrap();
        t.save(tmp.path()).unwrap();
        assert_eq!(
            resolve_tool_visibility(tmp.path(), crate::InteractionMode::Orchestrate),
            ToolVisibility::Orchestrator
        );
        assert_eq!(
            resolve_tool_visibility(tmp.path(), crate::InteractionMode::Work),
            ToolVisibility::NodeExecution
        );
    }
}
