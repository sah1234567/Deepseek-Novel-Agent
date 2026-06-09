//! AskUserQuestion tool: pauses the turn for user input. In Unattended mode the
//! model self-answers; in other modes it emits `ToolError::NeedsUserInput`.

use crate::{PermissionMode, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestion {
    pub id: String,
    pub prompt: String,
    pub options: Vec<AskQuestionOption>,
    #[serde(default)]
    pub allow_multiple: bool,
    #[serde(default)]
    pub allow_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionPayload {
    pub questions: Vec<AskQuestion>,
}

pub struct AskUserQuestionTool;

fn parse_questions(input: &Value) -> Result<AskUserQuestionPayload, ToolError> {
    let questions_val = input.get("questions").ok_or_else(|| {
        ToolError::Validation(crate::ValidationError::MissingField("questions".into()))
    })?;
    let questions: Vec<AskQuestion> = serde_json::from_value(questions_val.clone())
        .map_err(|e| ToolError::Validation(crate::ValidationError::InvalidField(e.to_string())))?;
    Ok(AskUserQuestionPayload { questions })
}

fn build_unattended_prompt(payload: &AskUserQuestionPayload) -> String {
    let mut out = String::new();
    out.push_str("[无人值守模式] 当前处于全自动创作模式。以下问题原本需要作者决策，现在需要你自行分析并做出最合理的选择。\n\n");

    out.push_str("## 决策前准备\n\n");
    out.push_str("在做出选择之前，请先利用当前对话上下文中已有的信息进行分析：\n");
    out.push_str(
        "- 回顾本轮已 Read 的人物卡演变日志、关系索引、大纲、细纲、伏笔追踪等知识库文件\n",
    );
    out.push_str("- 回顾已完成的章节正文（尤其是最近几章），理解已有剧情铺垫和人物状态\n");
    out.push_str("- 如果上下文中缺少做出判断所需的关键信息，可 Read 相关文件后再决策\n\n");

    out.push_str("## 决策维度\n\n");
    out.push_str("综合分析以下维度：\n");
    out.push_str(
        "1. **已有剧情铺垫** — 前文已建立的人物性格、关系走向、情节伏笔，选择应与已有铺垫一致\n",
    );
    out.push_str("2. **题材惯例与读者期待** — 当前作品所属题材的叙事惯例和目标读者的合理期待\n");
    out.push_str(
        "3. **后续剧情发展空间** — 哪个选项能为后续章节提供更丰富的冲突、转折和人物成长空间\n",
    );
    out.push_str("4. **叙事价值与戏剧张力** — 哪个选项在文学性和阅读体验上更有价值\n");
    out.push_str("5. **人物弧光一致性** — 选择应服务于主要人物的成长弧线，避免人物性格断裂\n");
    out.push_str("6. **世界观自洽** — 选择应与已建立的世界观规则和设定体系保持一致\n");

    for (i, q) in payload.questions.iter().enumerate() {
        out.push_str(&format!(
            "\n---\n## 问题 {}: {}\n\n选项:\n",
            i + 1,
            q.prompt
        ));
        for opt in &q.options {
            out.push_str(&format!("- {}\n", opt.label));
        }
        if q.allow_multiple {
            out.push_str("(可多选，选择 1~3 项)\n");
        }
        if q.allow_custom {
            out.push_str("(此题允许自定义输入，可不选预设选项，直接给出你的答案)\n");
        }
    }

    out.push_str("\n---\n\n## 输出格式\n\n");
    out.push_str("请在回复中按以下格式输出你的决策：\n");
    out.push_str("**问题 N**: 选择「xxx」，理由：...（1-2 句）\n");
    out.push_str("（若为自定义输入题，格式为：**问题 N**: 自定义答案：...，理由：...）\n\n");
    out.push_str("然后继续执行后续步骤。");
    out
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn description(&self) -> &str {
        "Ask the user to choose among creative options before proceeding"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "prompt": {"type": "string"},
                            "allow_multiple": {"type": "boolean"},
                            "allow_custom": {"type": "boolean"},
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": {"type": "string"},
                                        "label": {"type": "string"}
                                    },
                                    "required": ["id", "label"]
                                }
                            }
                        },
                        "required": ["id", "prompt", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn allowed_in_plan_mode(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let payload = parse_questions(&input)?;
        let mode = ctx.effective_permission_mode();
        if matches!(mode, PermissionMode::Unattended) {
            return Ok(ToolOutput {
                content: build_unattended_prompt(&payload),
                is_error: false,
            });
        }
        Err(ToolError::NeedsUserInput { payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolError};
    use tempfile::TempDir;

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    fn sample_payload() -> serde_json::Value {
        json!({
            "questions": [{
                "id": "q1",
                "prompt": "主角性别？",
                "options": [
                    {"id": "male", "label": "男"},
                    {"id": "female", "label": "女"}
                ],
                "allow_multiple": false
            }]
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn normal_mode_returns_needs_user_input() {
        let tmp = TempDir::new().unwrap();
        let ctx = test_ctx(&tmp);
        let err = AskUserQuestionTool
            .call(sample_payload(), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NeedsUserInput { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unattended_mode_returns_prompt() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Unattended,
            ..test_ctx(&tmp)
        };
        let out = AskUserQuestionTool
            .call(sample_payload(), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("无人值守模式"));
        assert!(out.content.contains("主角性别？"));
        assert!(out.content.contains("男"));
        assert!(out.content.contains("女"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unattended_prompt_includes_multiple_questions() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Unattended,
            ..test_ctx(&tmp)
        };
        let input = json!({
            "questions": [
                {"id": "q1", "prompt": "CP走向？", "options": [
                    {"id": "a", "label": "单女主"}, {"id": "b", "label": "后宫"}
                ]},
                {"id": "q2", "prompt": "结局倾向？", "options": [
                    {"id": "happy", "label": "圆满"}, {"id": "sad", "label": "悲剧"}
                ], "allow_multiple": false}
            ]
        });
        let out = AskUserQuestionTool.call(input, &ctx).await.unwrap();
        assert!(out.content.contains("CP走向？"));
        assert!(out.content.contains("结局倾向？"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unattended_prompt_marks_multi_select() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Unattended,
            ..test_ctx(&tmp)
        };
        let input = json!({
            "questions": [{
                "id": "q1",
                "prompt": "选择配角组合",
                "options": [
                    {"id": "x", "label": "角色A"},
                    {"id": "y", "label": "角色B"},
                    {"id": "z", "label": "角色C"}
                ],
                "allow_multiple": true
            }]
        });
        let out = AskUserQuestionTool.call(input, &ctx).await.unwrap();
        assert!(out.content.contains("可多选"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_questions_field_errors() {
        let tmp = TempDir::new().unwrap();
        let ctx = test_ctx(&tmp);
        let err = AskUserQuestionTool.call(json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("questions"));
    }
}
