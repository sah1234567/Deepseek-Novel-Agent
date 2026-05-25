use novel_knowledge::{
    append_evolution_log, parse_frontmatter, CharacterFrontmatter, KnowledgeStore,
};
use tempfile::TempDir;

#[test]
fn min_project_knowledge_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let store = KnowledgeStore::new(tmp.path());
    let card = r#"---
name: 林若烟
aliases: [若烟]
category: human
firstAppearance: chapter-001
lastUpdate: chapter-001
status: alive
povCharacter: true
---

## 出场记录日志
| 章节 | 关键事件 | 伏笔关联 | 情绪弧线 |
|------|---------|---------|---------|
| Ch1  | 入门测试 | F03     | 好奇    |
"#;
    store
        .write_file("knowledge/characters/林若烟.md", card)
        .unwrap();
    let read = store.read_file("knowledge/characters/林若烟.md").unwrap();
    let (fm, _): (CharacterFrontmatter, _) = parse_frontmatter(&read).unwrap();
    assert_eq!(fm.name, "林若烟");
    let updated = append_evolution_log(&read, "出场记录日志", "| Ch2 | 修炼 | — | 专注 |").unwrap();
    store
        .write_file("knowledge/characters/林若烟.md", &updated)
        .unwrap();
    let final_content = store.read_file("knowledge/characters/林若烟.md").unwrap();
    assert!(final_content.contains("Ch2"));
}
