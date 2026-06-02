use super::common::{
    default_outline_column_map, in_chapter_range, list_outline_files, outline_col_value,
    parse_chapter_num, parse_outline_with_header,
};
use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
pub(crate) struct GridRow {
    chapter: String,
    scene: String,
    pov_character: String,
    timeline_point: String,
    tension_level: u8,
    foreshadowings: Vec<String>,
    core_event: String,
    /// Value of the first column (structure unit: 卷/世界/副本 etc). Name comes from header.
    structure_unit: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlotGridResult {
    grid: Vec<GridRow>,
    dimensions: Vec<String>,
}

const ALL_DIMENSIONS: &[&str] = &[
    "scene",
    "pov",
    "timeline",
    "tension",
    "foreshadowing",
    "event",
];

pub struct PlotGridTool;

fn parse_chapter_range(input: &Value) -> Option<(u32, u32)> {
    let arr = input.get("chapter_range")?.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    Some((arr[0].as_u64()? as u32, arr[1].as_u64()? as u32))
}

fn parse_outline_row(
    cells: &[String],
    colmap: &super::common::OutlineColumnMap,
    current_unit: &str,
) -> Option<(u32, GridRow)> {
    // Find chapter number: try "章" column first, then "章节", then cells[1] as fallback
    let chapter_num = outline_col_value(cells, colmap, "章")
        .or_else(|| outline_col_value(cells, colmap, "章节"))
        .or_else(|| cells.get(1).map(|s| s.as_str()))
        .and_then(|s| {
            let n = parse_chapter_num(s);
            if n > 0 {
                Some(n)
            } else {
                s.parse().ok()
            }
        })?;
    if chapter_num == 0 {
        return None;
    }
    // Structure unit: first column value, or current heading context
    let unit = cells
        .first()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "—" && s != "-")
        .unwrap_or_else(|| current_unit.to_string());
    let tension = outline_col_value(cells, colmap, "张力")
        .and_then(|s| s.chars().find(|c| c.is_ascii_digit()))
        .and_then(|c| c.to_digit(10))
        .unwrap_or(3) as u8;
    let foreshadowings = outline_col_value(cells, colmap, "需推进的伏笔")
        .or_else(|| outline_col_value(cells, colmap, "伏笔"))
        .map(|s| {
            s.split([',', '，', ';', '；'])
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty() && x != "—" && x != "-")
                .collect()
        })
        .unwrap_or_default();
    Some((
        chapter_num,
        GridRow {
            chapter: format!("Ch{chapter_num}"),
            scene: outline_col_value(cells, colmap, "章节标题")
                .unwrap_or("")
                .to_string(),
            pov_character: outline_col_value(cells, colmap, "pov")
                .unwrap_or("—")
                .to_string(),
            timeline_point: outline_col_value(cells, colmap, "时间点")
                .unwrap_or("—")
                .to_string(),
            tension_level: tension.clamp(1, 5),
            foreshadowings,
            core_event: outline_col_value(cells, colmap, "核心事件")
                .unwrap_or("")
                .to_string(),
            structure_unit: unit,
        },
    ))
}

fn parse_detailed_outline(content: &str, chapter_num: u32) -> GridRow {
    let mut row = GridRow {
        chapter: format!("Ch{chapter_num}"),
        scene: String::new(),
        pov_character: "—".into(),
        timeline_point: "—".into(),
        tension_level: 3,
        foreshadowings: vec![],
        core_event: String::new(),
        structure_unit: String::new(),
    };
    for line in content.lines() {
        if line.contains("POV") && line.contains('✓') {
            if let Some(name) = line.split('|').nth(1) {
                row.pov_character = name.trim().to_string();
            }
        }
        if line.starts_with("### 场景") {
            row.scene = line.trim_start_matches('#').trim().to_string();
        }
        // Tension level is left at default (3); the Agent interprets
        // emotional context from the detailed outline text via its prompt
        if line.contains('F') && (line.contains("伏笔") || line.contains('|')) {
            for token in line.split(|c: char| !c.is_ascii_alphanumeric()) {
                if token.starts_with('F') && token.len() <= 4 {
                    row.foreshadowings.push(token.to_string());
                }
            }
        }
    }
    if row.core_event.is_empty() {
        row.core_event = content
            .lines()
            .find(|l| l.contains("核心事件") || l.contains("本章目标"))
            .unwrap_or("")
            .to_string();
    }
    row
}

#[async_trait]
impl Tool for PlotGridTool {
    fn name(&self) -> &str {
        "PlotGrid"
    }
    fn description(&self) -> &str {
        "Multi-dimensional plot grid from outline and detailed outlines"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter_range": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "minItems": 2,
                    "maxItems": 2
                },
                "dimensions": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["scene", "pov", "timeline", "tension", "foreshadowing", "event"]
                    }
                }
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let range = parse_chapter_range(&input);
        let dimensions: Vec<String> = input
            .get("dimensions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| ALL_DIMENSIONS.iter().map(|s| s.to_string()).collect());
        let result = build_plot_grid(&ctx.project_root, range, dimensions)?;
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::Execution(format!("json serialize: {e}")))?,
            is_error: false,
        })
    }
}

pub(crate) fn build_plot_grid(
    project_root: &std::path::Path,
    range: Option<(u32, u32)>,
    dimensions: Vec<String>,
) -> Result<PlotGridResult, ToolError> {
    let store = KnowledgeStore::new(project_root);
    let mut grid_map: std::collections::BTreeMap<u32, GridRow> = std::collections::BTreeMap::new();

    if let Ok(outline) = store.read_file("knowledge/plot/大纲.md") {
        let (colmap, current_unit, data_rows) = parse_outline_with_header(&outline);
        let map: &super::common::OutlineColumnMap = match &colmap {
            Some(m) => m,
            None => default_outline_column_map(),
        };
        for cells in data_rows {
            if let Some((num, row)) = parse_outline_row(&cells, map, &current_unit) {
                if in_chapter_range(num, range) {
                    grid_map.insert(num, row);
                }
            }
        }
    }

    for (num, path) in list_outline_files(project_root) {
        if !in_chapter_range(num, range) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            let detail = parse_detailed_outline(&content, num);
            grid_map
                .entry(num)
                .and_modify(|r| {
                    if r.scene.is_empty() {
                        r.scene = detail.scene.clone();
                    }
                    if r.pov_character == "—" {
                        r.pov_character = detail.pov_character.clone();
                    }
                    if r.foreshadowings.is_empty() {
                        r.foreshadowings = detail.foreshadowings.clone();
                    }
                    if r.core_event.is_empty() {
                        r.core_event = detail.core_event.clone();
                    }
                    if detail.tension_level > r.tension_level {
                        r.tension_level = detail.tension_level;
                    }
                })
                .or_insert(detail);
        }
    }

    let mut grid: Vec<GridRow> = grid_map.into_values().collect();
    apply_dimension_masks(&mut grid, &dimensions);

    Ok(PlotGridResult {
        grid,
        dimensions: dimensions
            .into_iter()
            .filter(|d| ALL_DIMENSIONS.contains(&d.as_str()))
            .collect(),
    })
}

pub(crate) fn apply_dimension_masks(grid: &mut [GridRow], dimensions: &[String]) {
    if !dimensions.iter().any(|d| d == "scene") {
        for row in &mut *grid {
            row.scene = "—".into();
        }
    }
    if !dimensions.iter().any(|d| d == "pov") {
        for row in &mut *grid {
            row.pov_character = "—".into();
        }
    }
    if !dimensions.iter().any(|d| d == "timeline") {
        for row in &mut *grid {
            row.timeline_point = "—".into();
        }
    }
    if !dimensions.iter().any(|d| d == "tension") {
        for row in &mut *grid {
            row.tension_level = 0;
        }
    }
    if !dimensions.iter().any(|d| d == "foreshadowing") {
        for row in &mut *grid {
            row.foreshadowings.clear();
        }
    }
    if !dimensions.iter().any(|d| d == "event") {
        for row in &mut *grid {
            row.core_event = "—".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[test]
    fn parse_detailed_outline_extracts_fields() {
        let content = "\
### 场景 山门试炼
POV ✓ | 林若烟 |
伏笔 F03 | 需推进
核心事件：测试灵根
";
        let row = parse_detailed_outline(content, 7);
        assert_eq!(row.chapter, "Ch7");
        assert!(row.scene.contains("场景"));
        assert_eq!(row.pov_character, "林若烟");
        assert!(row.foreshadowings.iter().any(|f| f == "F03"));
        assert!(row.core_event.contains("测试灵根"));
    }

    #[test]
    fn apply_dimension_masks_clears_scene() {
        let mut grid = vec![GridRow {
            chapter: "Ch1".into(),
            scene: "山门".into(),
            pov_character: "甲".into(),
            timeline_point: "春".into(),
            tension_level: 3,
            foreshadowings: vec!["F1".into()],
            core_event: "入门".into(),
            structure_unit: "卷一".into(),
        }];
        apply_dimension_masks(&mut grid, &["pov".into()]);
        assert_eq!(grid[0].scene, "—");
        assert_eq!(grid[0].pov_character, "甲");
    }

    #[test]
    fn build_plot_grid_filters_dimensions() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/大纲.md"),
            "| 卷 | 章 | 章节标题 | 核心事件 | 需推进的伏笔 | 张力 | POV | 时间点 |\n\
             |----|-----|---------|---------|------------|-----|-----|-------|\n\
             | 一 | 1 | 入门 | 测试灵根 | F03 | 3 | 林若烟 | 春 |\n",
        )
        .unwrap();
        let result =
            build_plot_grid(tmp.path(), None, vec!["pov".into()]).expect("build_plot_grid");
        assert_eq!(result.dimensions, vec!["pov"]);
        assert_eq!(result.grid[0].scene, "—");
        assert_eq!(result.grid[0].pov_character, "林若烟");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plot_grid_from_outline() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/大纲.md"),
            "| 卷 | 章 | 章节标题 | 核心事件 | 需推进的伏笔 | 张力 | POV | 时间点 |\n\
             |----|-----|---------|---------|------------|-----|-----|-------|\n\
             | 一 | 1 | 入门 | 测试灵根 | F03 | 3 | 林若烟 | 春 |\n",
        )
        .unwrap();
        let tool = PlotGridTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool.call(json!({}), &ctx).await.unwrap();
        assert!(out.content.contains("林若烟"));
        assert!(out.content.contains("F03"));
    }
}
