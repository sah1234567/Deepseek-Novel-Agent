---
name: publish-checklist
description: 发布前检查——平台适配（章节字数合规、标题/简介、敏感内容提示、断更风险）+ 全书完整性（伏笔全回收、人物弧线完整、因果链无断头）。当用户要发布/完结/发书时使用。触发词："发布"、"发书"、"完结"、"上架"。
when_to_use: 用户要求发布、上架或确认完结时使用
skill_kind: workflow
allowed-tools: Read, Grep, Glob, Stats, ForeshadowTracker, WorkHealthCheck, CharacterSearch, RelationQuery, TrackingQuery, TodoWrite, AskUserQuestion
---

# publish-checklist — 发布前检查

用户要求发布/上架/确认完结时执行。**只读检查 + 汇报清单**，不直接改稿（改稿按报告由用户或后续指令执行）。

## 第一部分：平台适配

| 检查项 | 标准 | 工具 |
|--------|------|------|
| 章节字数合规 | 每章 2000–4000 字（[chapter-writing](../chapter-writing/SKILL.md) 标准）；偏差章节列出 | `Stats(chapter="all")` + 偏差章 `Stats(chapter="N")` |
| 标题与简介 | 全书标题（AGENTS.md / 大纲首行）、卷标题、简介/文案草拟建议 | Read `knowledge/plot/大纲.md` 头部 |
| 敏感内容提示 | 按当前题材 Skill 的敏感区（血腥/政治/宗教等）逐项核对，列出提示 | Read 题材 Skill 的「常见错误」节 |
| 断更风险提示 | 剩余细纲覆盖 ≥ 5 章、大纲未写卷数、Loop 剩余站数 | Read `knowledge/plot/细纲/`、`knowledge/plot/大纲.md` |

## 第二部分：全书完整性

| 检查项 | 标准 | 工具 |
|--------|------|------|
| 伏笔回收 | 待回收伏笔为 0（或已明确废弃并标注） | `ForeshadowTracker(current_chapter=ChN)`；回收率 <100% 列出剩余清单 |
| 人物弧线完整 | 主要人物卡「性格演变日志」末行无悬空（角色未完成弧线需标注） | `CharacterSearch` + Read 人物卡演变日志末行 |
| 因果链闭合 | 无孤立事件（无前因无后果） | `WorkHealthCheck`（含因果断头检查） |
| 审计状态 | 全章 PA/KA/CCA 无待处理项 | `AuditStatusQuery(operation=summary)` |

## 输出结构

1. **摘要** — 是否可以发布（可直接发布 / 需处理 N 项后再发布）
2. **平台适配** — 逐项结果（✓ / ⚠️ 需处理）
3. **全书完整性** — 逐项结果（✓ / ⚠️ 需处理），⚠️ 项附文件路径与建议
4. **发布建议** — 标题/简介文案建议、首卷发布范围建议
5. **`## 接下来（主 Agent 必读）`** — 待用户拍板的项（如需删除/废弃伏笔、是否拆分章节）

## 约束

- 只读检查；修改动作由用户确认后执行
- 伏笔回收率未达 100% 时，区分「已废弃（合法）」与「未回收（风险）」
- 不要代替用户决定「是否删除某伏笔」——列选项交用户
