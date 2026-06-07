---
name: post-chapter-checklist
description: 正文写后收尾核对——确认细纲收尾记录、执行写后 2 项 Subagent 审计。独立使用场景：作者自己写完正文后的收尾核对、续写前的状态复查、chapter-writing 中跳过审计时的补救。触发词："写后检查"、"收尾"、"核对知识库"、"检查第X章"
when_to_use: 章节正文已 Write 后做收尾核对。写前准备或纯策划不必 Invoke。
skill_kind: workflow
allowed-tools: Read, Edit, Glob, Grep, CharacterSearch, Tail, TodoWrite, ForkSubAgent, TrackingQuery, RelationQuery, ForeshadowTracker, Stats
---

# 正文写后收尾清单

## 阶段说明

本 Skill 在**正文已 Write** 后使用。**追踪文件更新已在细纲阶段完成**——本 Skill 仅核对正文写后的收尾记录与审计，不涉及全量知识库更新。若尚未写正文，先 InvokeSkill(chapter-writing)。

## 收尾记录核对

逐项确认本章收尾记录已完成：

| 检查项 | 方法 | 期望 |
|--------|------|------|
| 字数 | `Stats(chapter="N")` | 本章 2000–4000 字 |
| 细纲「写后记录」 | Read `knowledge/plot/细纲/chapter-NNN-细纲.md` | 「字数」「核心事件」「钩子」已填 |
| 细纲「知识库更新确认」 | 同上 | 逐条标记（已按计划执行 / 有偏差 / 无变化） |
| 执行偏差处理 | 同上 | 如有偏差 →「修订记录」有对应行 + 相关追踪文件已修正 |
| 大纲章节索引 | Read 大纲对应卷索引段 `\| N \|` | 本章行存在且「核心事件」与正文一致 |
| INDEX.md 进度 | Read `knowledge/INDEX.md` | 进度段已更新到本章 |
| 审计台账 | Read `knowledge/meta/audit-status.md` 或 AuditStatusQuery | 本章 KA/CCA 已标已通过（修复后） |
| 角色出场 | `CharacterRotate` | 是否有重要角色连续 ≥5 章未出场 |

遗漏项 → Edit 补充。

## 写后 Subagent 审计（必做，2 项同批并行）

收尾记录核对完成后，**必须**在同一次 assistant 消息内 Fork 以下 Subagent（不可跳过、不可只做其中一项）：

| # | agentType | task 要点 |
|---|-----------|-----------|
| 1 | KnowledgeAuditor | `审计 chapters/chapter-NNN.md 是否忠实执行细纲场景拆分；是否有意外事件需补充登记；收尾是否完整` |
| 2 | ChapterCraftAnalyzer | `分析 chapters/chapter-NNN.md：对话质量、叙事节奏、情感轨迹、设定一致性（称呼/POV边界/战力/场景道具）、反AI味指标。POV=XXX` |

按全部报告 Edit 修复；需作者决策的项用 AskUserQuestion。

**禁止** 跳过上述 Fork 或只跑其中一项即向作者宣告完成。

## 本阶段完成后

1. 知识库收尾已确认且 **2 项 Subagent 均已跑完**：汇总场景忠实度、对话/节奏/情感/设定一致性结论
2. 全部修复完成后，才可回复用户「本章完成」
3. **不要**建议主 Agent 在正文后重新全量更新追踪文件——那应在细纲阶段完成

**下一步：** 如需继续写下一章 → InvokeSkill(`chapter-writing`)。如审计发现需修改正文 → InvokeSkill(`revision`)。如一切正常 → 回复作者「本章完成」。
