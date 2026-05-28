---
name: post-chapter-checklist
description: 写后收尾清单——核对知识库是否已同步、执行写后 2 项 Subagent 审计。当章节正文已 Write 后做收尾核对时使用。触发词："写后检查"、"知识库更新"、"收尾"、"核对知识库"
when_to_use: 章节正文已 Write 后做收尾核对。写前准备或纯策划不必 Invoke。
skill_kind: workflow
allowed-tools: Read, Edit, Glob, Grep, CharacterSearch, Tail, TodoWrite, ForkSubAgent, TrackingQuery, RelationQuery, ForeshadowTracker, Stats
---

# 写后收尾清单

## 阶段说明

本 Skill 在**正文已 Write** 后使用，核对 knowledge/ 是否与章节同步。不负责写正文——若尚未写章，先 InvokeSkill(chapter-writing)。

## 知识库同步核对清单

逐项确认本章相关条目已 append。**优先用专用工具**，Grep/Read 为后备：

| 检查项 | 首选工具 | 期望 |
|--------|---------|------|
| 字数 | `Stats(chapter="N")` | 本章 2000–4000 字 |
| 细纲完成度 | Read `knowledge/plot/细纲/chapter-NNN-细纲.md` | 「实际完成」「知识库更新确认」已填 |
| 时间线 | `TrackingQuery(file="timeline", operation="current")` | 末行章节号 ≥ 本章 |
| 场景 | `TrackingQuery(file="scene", operation="current")` | 涉及场景末行章节号 ≥ 本章（如有变化） |
| 道具 | `TrackingQuery(file="prop", operation="current")` | 道具末行章节号 ≥ 本章（如有变化） |
| 势力 | `TrackingQuery(file="faction", operation="current")` | 势力末行章节号 ≥ 本章（如有变化） |
| 人物出场 | 各出场人物卡，Grep `^\| Ch` | 出场记录日志末行 ≥ 本章 |
| 关系/称呼 | `RelationQuery(character="出场人物名")` | 当前关系/称呼与正文一致；如有变化，末行章节号 ≥ 本章 |
| 伏笔 | `ForeshadowTracker(current_chapter="ChN")` | urgent 列表不含本章应回收的伏笔 |
| 因果 | Read `knowledge/plot/因果链.md` | 新因果边已追加（如有） |
| 进度 | Read `knowledge/INDEX.md` | 最后出场章、进度段已更新 |
| Todo | TodoWrite | 本章 marked completed |

遗漏项 → Edit append（演变日志只 append，不覆写已有行）。

## 写后 Subagent 审计（必做，2 项同批并行）

知识库手动核对完成后，**必须**在同一次 assistant 消息内 Fork 以下 Subagent（不可跳过、不可只做其中一项）：

| # | agentType | task 要点 |
|---|-----------|-----------|
| 1 | KnowledgeAuditor | `chapters/chapter-NNN.md` 知识库遗漏 + 设定一致性深度审计 |
| 2 | ChapterCraftAnalyzer | 对话质量、叙事节奏、本章 POV/主角情感轨迹 |

按全部报告 Edit 修复；需作者决策的项用 AskUserQuestion。

**禁止** 跳过上述 Fork 或只跑其中一项即向作者宣告完成。

## 本阶段完成后

1. 知识库已同步且 **2 项 Subagent 均已跑完**：汇总遗漏扫描、一致性、对话/节奏/情感结论。
2. 全部修复完成后，才可回复用户「本章完成」。
