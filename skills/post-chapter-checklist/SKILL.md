---
name: post-chapter-checklist
description: 写后收尾清单——核对知识库是否已同步、执行写后 5 项 Subagent 审计。当章节正文已 Write 后做收尾核对时使用。触发词："写后检查"、"知识库更新"、"收尾"、"核对知识库"
when_to_use: 章节正文已 Write 后做收尾核对。写前准备或纯策划不必 Invoke。
skill_kind: workflow
allowed-tools: Read, Edit, Glob, Grep, CharacterSearch, TodoWrite, ForkSubAgent
---

# 写后收尾清单

## 阶段说明

本 Skill 在**正文已 Write** 后使用，核对 knowledge/ 是否与章节同步。不负责写正文——若尚未写章，先 InvokeSkill(chapter-writing)。

## 知识库同步核对清单

逐项 Read 并确认本章相关条目已 append：

| 检查项 | 文件 | 期望 |
|--------|------|------|
| 细纲完成度 | knowledge/plot/细纲/chapter-NNN-细纲.md | 「实际完成」「知识库更新确认」已填 |
| 时间线 | knowledge/shared-systems/时间线.md | 本章时间戳已追加 |
| 场景 | knowledge/shared-systems/场景追踪.md | 涉及场景状态已追加（如有） |
| 道具 | knowledge/shared-systems/道具追踪.md | 道具归属/状态已追加（如有） |
| 人物出场 | 各出场人物卡 | 出场记录日志末行 ≥ 本章 |
| 关系/称呼 | _关系与称呼索引.md | 双向称呼/关系已追加（如有变化） |
| 伏笔 | knowledge/plot/伏笔追踪.md | 推进/回收/新埋已记录 |
| 因果 | knowledge/plot/因果链.md | 新因果边已追加（如有） |
| 进度 | knowledge/INDEX.md | 最后出场章、进度段已更新 |
| Todo | TodoWrite | 本章 marked completed |

遗漏项 → Edit append（演变日志只 append，不覆写已有行）。

## 写后 Subagent 审计（必做，5 项同批并行）

知识库手动核对完成后，**必须**在同一次 assistant 消息内 Fork 以下 Subagent（不可跳过、不可只做其中几项）：

| # | agentType | task 要点 |
|---|-----------|-----------|
| 1 | LogIntegrityChecker | `chapters/chapter-NNN.md` 知识库更新遗漏 |
| 2 | ConsistencyChecker | 深度 9 维设定一致性审计 |
| 3 | DialogueAnalyzer | 对话质量与 AI 味对白 |
| 4 | PacingAnalyzer | 叙事节奏 |
| 5 | EmotionAnalyzer | 本章 POV/主角情感轨迹 |

按全部报告 Edit 修复；需作者决策的项用 AskUserQuestion。

**禁止** 仅用 ConsistencyCheck 工具代替上述 Fork。

## 本阶段完成后

1. 知识库已同步且 **5 项 Subagent 均已跑完**：汇总遗漏扫描、一致性、对话、节奏、情感结论。
2. 全部修复完成后，才可回复用户「本章完成」。
