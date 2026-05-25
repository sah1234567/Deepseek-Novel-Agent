use super::common::{count_chinese_chars, list_character_names, parse_chapter_num};
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

/// Raw data collected for an aspect — the Agent reads this and makes its own judgment.
#[derive(Debug, Serialize)]
struct AspectData {
    aspect: String,
    chapter_content: String,
    chapter_word_count: u32,
    /// Relevant KB file contents keyed by relative path.
    files: Vec<(String, String)>,
    /// Last rows of relevant log tables for quick scanning.
    last_rows: Vec<(String, String, String)>, // (file, table_name, last_row)
    /// Character names found in the chapter.
    characters_in_chapter: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DataCollectionResult {
    chapter_path: String,
    chapter_num: u32,
    aspects: Vec<AspectData>,
}

pub struct ConsistencyCheckTool;

const ALL_ASPECTS: &[&str] = &[
    "characters",
    "timeline",
    "powerSystem",
    "foreshadowing",
    "viewpoint",
    "titles",
    "scene",
    "prop",
    "system",
    "wordCount",
];

fn parse_aspects(input: &Value) -> Vec<String> {
    input
        .get("aspects")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| ALL_ASPECTS.iter().map(|s| s.to_string()).collect())
}

/// Collect raw data for an aspect — no judgments, no issue detection.
fn collect_aspect_data(
    aspect: &str,
    chapter: &str,
    _chapter_path: &str,
    _current_ch: u32,
    store: &KnowledgeStore,
    known_names: &[String],
) -> AspectData {
    let mut files = Vec::new();
    let mut last_rows = Vec::new();
    let chapter_word_count = count_chinese_chars(chapter);
    let found = super::common::extract_names_in_text(chapter, known_names);

    // Read KB files relevant to this aspect and extract last rows of log tables
    match aspect {
        "characters" => {
            for name in &found {
                let rel = format!("knowledge/characters/{name}.md");
                if let Ok(card) = store.read_file(&rel) {
                    for table in &["出场记录日志", "身份演变日志", "性格演变日志"] {
                        if let Ok(Some(row)) =
                            novel_knowledge::find_table_last_row(&card, table)
                        {
                            last_rows.push((rel.clone(), table.to_string(), row));
                        }
                    }
                    files.push((rel, card));
                }
            }
        }
        "timeline" => {
            let path = "knowledge/shared-systems/时间线.md";
            if let Ok(content) = store.read_file(path) {
                if let Some(row) = super::common::last_data_row(&content, "时间线演变日志")
                {
                    last_rows.push((path.to_string(), "时间线演变日志".into(), row));
                }
                files.push((path.to_string(), content));
            }
        }
        "powerSystem" => {
            let path = "knowledge/shared-systems/功法技能.md";
            if let Ok(content) = store.read_file(path) {
                files.push((path.to_string(), content));
            }
            for name in &found {
                let rel = format!("knowledge/characters/{name}.md");
                if let Ok(card) = store.read_file(&rel) {
                    if let Ok(Some(row)) =
                        novel_knowledge::find_table_last_row(&card, "功法演变日志")
                    {
                        last_rows.push((rel.clone(), "功法演变日志".into(), row));
                    }
                }
            }
        }
        "foreshadowing" => {
            let path = "knowledge/plot/伏笔追踪.md";
            if let Ok(content) = store.read_file(path) {
                files.push((path.to_string(), content));
            }
        }
        "viewpoint" => {
            for name in &found {
                let rel = format!("knowledge/characters/{name}.md");
                if let Ok(card) = store.read_file(&rel) {
                    if let Ok(Some(row)) =
                        novel_knowledge::find_table_last_row(&card, "已知信息演变日志")
                    {
                        last_rows.push((rel.clone(), "已知信息演变日志".into(), row));
                    }
                }
            }
            let scene_path = "knowledge/shared-systems/场景追踪.md";
            if let Ok(content) = store.read_file(scene_path) {
                files.push((scene_path.to_string(), content));
            }
        }
        "titles" => {
            let path = "knowledge/characters/_关系与称呼索引.md";
            if let Ok(content) = store.read_file(path) {
                files.push((path.to_string(), content));
            }
        }
        "scene" => {
            let path = "knowledge/shared-systems/场景追踪.md";
            if let Ok(content) = store.read_file(path) {
                files.push((path.to_string(), content));
            }
        }
        "prop" => {
            let path = "knowledge/shared-systems/道具追踪.md";
            if let Ok(content) = store.read_file(path) {
                files.push((path.to_string(), content));
            }
        }
        "system" => {
            for name in &found {
                let rel = format!("knowledge/characters/{name}.md");
                if let Ok(card) = store.read_file(&rel) {
                    if card.contains("category: system") {
                        if let Ok(Some(row)) =
                            novel_knowledge::find_table_last_row(&card, "性格演变日志")
                        {
                            last_rows.push((rel.clone(), "性格演变日志".into(), row));
                        }
                        files.push((rel, card));
                    }
                }
            }
        }
        "wordCount" => {
            // Pure data — word count is already in chapter_word_count
        }
        _ => {}
    }

    AspectData {
        aspect: aspect.to_string(),
        chapter_content: chapter.to_string(),
        chapter_word_count,
        files,
        last_rows,
        characters_in_chapter: found,
    }
}

#[async_trait]
impl Tool for ConsistencyCheckTool {
    fn name(&self) -> &str {
        "ConsistencyCheck"
    }
    fn description(&self) -> &str {
        "Collect chapter and KB data for consistency review. \
         The Agent reads the returned data and makes its own judgment — \
         this tool does not detect issues or assign severity."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter_path": {"type": "string"},
                "aspects": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["characters", "timeline", "powerSystem", "foreshadowing", "viewpoint", "titles", "scene", "prop", "system", "wordCount"]
                    }
                }
            },
            "required": ["chapter_path"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let chapter_path = require_str(&input, "chapter_path")?;
        let aspects = parse_aspects(&input);
        let full = ctx.resolve_path(&chapter_path);
        let chapter = crate::blocking::read_to_string(full)
            .await
            .map_err(|e| ToolError::Execution(format!("cannot read chapter: {e}")))?;
        let current_ch = parse_chapter_num(&chapter_path);

        let store = KnowledgeStore::new(&ctx.project_root);
        let kb_root = ctx.project_root.join("knowledge");
        if !kb_root.exists() {
            return Err(ToolError::Execution(
                "KnowledgeBaseNotFound: knowledge/ directory missing".into(),
            ));
        }

        let known_names = list_character_names(&store);
        let mut collected = Vec::new();

        for aspect in &aspects {
            collected.push(collect_aspect_data(
                aspect,
                &chapter,
                &chapter_path,
                current_ch,
                &store,
                &known_names,
            ));
        }

        let result = DataCollectionResult {
            chapter_path: chapter_path.to_string(),
            chapter_num: current_ch,
            aspects: collected,
        };
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::Internal(e.to_string()))?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    fn setup_kb(tmp: &TempDir) {
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/shared-systems")).unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
    }

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_chapter_no_issues() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);
        std::fs::write(tmp.path().join("chapters/empty.md"), "").unwrap();
        let tool = ConsistencyCheckTool;
        let out = tool
            .call(
                json!({"chapter_path": "chapters/empty.md", "aspects": ["characters", "timeline"]}),
                &test_ctx(&tmp),
            )
            .await
            .unwrap();
        assert!(out.content.contains("aspects"));
        assert!(out.content.contains("chapter_word_count"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn title_data_collected() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);
        std::fs::write(
            tmp.path().join("knowledge/characters/_关系与称呼索引.md"),
            "| 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |\n\
             |------|--------|------|--------|--------|--------------|---------------|----------|\n\
             | Ch3 | 林若烟 | 陈默 | — | 疏远 | —→\"陈前辈\" | —→\"丫头\" | 初见 |\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "---\nname: 林若烟\ncategory: human\n---\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/陈默.md"),
            "---\nname: 陈默\ncategory: human\n---\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapters/chapter-031.md"),
            "林若烟喊道：「陈默！站住！」",
        )
        .unwrap();
        let tool = ConsistencyCheckTool;
        let out = tool
            .call(
                json!({"chapter_path": "chapters/chapter-031.md", "aspects": ["titles"]}),
                &test_ctx(&tmp),
            )
            .await
            .unwrap();
        assert!(out.content.contains("titles"));
        assert!(out.content.contains("_关系与称呼索引"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn foreshadowing_data_collected() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);
        std::fs::write(
            tmp.path().join("knowledge/plot/伏笔追踪.md"),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch5 | F01 | 埋设 | 陈默左手伤疤发光 | 待回收 | Ch35 | 陈默 |\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapters/chapter-035.md"),
            "Chapter 35: 决战时刻。",
        )
        .unwrap();
        let tool = ConsistencyCheckTool;
        let out = tool
            .call(
                json!({"chapter_path": "chapters/chapter-035.md", "aspects": ["foreshadowing"]}),
                &test_ctx(&tmp),
            )
            .await
            .unwrap();
        assert!(out.content.contains("伏笔追踪"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn viewpoint_data_collected() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "## 已知信息演变日志\n| 章节 | 信息 | 知晓程度 | 来源 |\n\
             |------|------|---------|------|\n\
             | Ch10 | 戒指上的古文字含义 | 未知 | — |\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapters/chapter-031.md"),
            "林若烟心想：原来那些古文字是上古秘境的坐标。",
        )
        .unwrap();
        let tool = ConsistencyCheckTool;
        let out = tool
            .call(
                json!({"chapter_path": "chapters/chapter-031.md", "aspects": ["viewpoint"]}),
                &test_ctx(&tmp),
            )
            .await
            .unwrap();
        assert!(out.content.contains("viewpoint"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn characters_data_includes_log_last_rows() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "## 出场记录日志\n| 章节 | 关键事件 | 伏笔 | 情绪 |\n|------|---------|------|------|\n| Ch1 | 入门 | — | 平 |\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapters/chapter-010.md"),
            "林若烟再次出场。",
        )
        .unwrap();
        let tool = ConsistencyCheckTool;
        let out = tool
            .call(
                json!({"chapter_path": "chapters/chapter-010.md", "aspects": ["characters"]}),
                &test_ctx(&tmp),
            )
            .await
            .unwrap();
        assert!(out.content.contains("last_rows"));
        assert!(out.content.contains("Ch1"));
    }
}
