---
name: revision
description: 修订与级联改稿工作流——ImpactAnalysis 定位影响范围、按优先级级联 Edit。含改大纲、改细纲、删章等分支。触发词："改稿"、"修订"、"修改设定"、"级联修改"、"改大纲"、"改细纲"、"删章"
when_to_use: 改稿/修设定/级联修改时使用。写新章、纯策划可忽略本 Skill。
skill_kind: workflow
allowed-tools: Read, Write, Edit, Glob, Grep, Bash, CharacterSearch, PlotGraph, ImpactAnalysis, Tail, InvokeSkill, AskUserQuestion, ForkSubAgent, TrackingQuery, RelationQuery, ForeshadowTracker
---

# 修订与级联改稿工作流

## 阶段说明

本 Skill 覆盖**改稿阶段** SOP。主 Agent 可直接 Edit `chapters/**` 与 knowledge/**。

## 第一步：影响分析（不可跳过）

1. 解析用户反馈，确定受影响范围（角色性格、战力体系、情节走向、世界观设定等）
2. 使用 **ImpactAnalysis** 获取级联影响概览，再用以下专用工具定位所有影响点。**优先用专用工具**——Grep/Read 仅当专用工具未覆盖或需精确行号时作为后备：
   - 人物卡出场记录日志 → `CharacterSearch` 找到相关人物卡
   - 追踪文件 → `TrackingQuery(file="...", operation="search", keyword="角色名")` 查场景/道具/势力/时间线/战力/功法中受影响的条目
   - 因果关系 → `PlotGraph(event="事件名", direction="both")` 追溯因果链
   - 伏笔追踪 → `ForeshadowTracker(character="角色名")` 查关联伏笔
   - 关系/称呼 → `RelationQuery(character="角色名", include_history=true)` 查所有关系变化
   定位完成后按清单逐文件分段 Read（改稿读章末衔接用 Tail；中间段用 Read offset/limit；禁止批量 full Read）
3. ★ **Read 受影响的知识库文件头注释获取该类型文件的修改规则**（伏笔追踪.md / 因果链.md / 人物卡 template 均有修改规则注释）。规则核心：**追加修正行，不删除不覆写旧行**。
4. 产出影响报告（列出受影响的：人物卡、大纲、细纲、正文章节、伏笔、因果链）
5. 影响范围超过 3 个文件时，使用 AskUserQuestion 向用户确认后再执行

## 第二步：级联修改（确认后）

修订正文时同步清除 AI 味：删改「不是…而是…」、破折号泛滥、「然后/首先/其次」堆砌、句长均匀、情感空标签、说明书式叙述；补入具体细节与闲笔，长短句交替。

**同文件连续改稿**：
- **逐处精确改**（`replace_all:false`）：`old_string` 须在 `committed_spans` 内（即 Read/Tail tool_result 已在对话中）。超出 → Grep 锚点 → Read offset/limit 扩窗 → 等结果进对话后再 Edit。同文件连续改无需每次重读；多段 Read 自动并集为 disjoint spans（如先读 80–100、再读 50–90 → 可改 50–100）。
- **全文批量改名**（`replace_all:true`）：仅需本会话 Read/Tail 过该文件（有 cache 即可，**不检查 committed span**）。Grep 确认匹配位置与数量 → Edit `replace_all:true`，盘上字节匹配代替全文 Read；Edit 后 cache 升为整文件。
- **同轮 Read→Edit 被拒**：tool_result 尚未进入 transcript（committed 未 promote）。**下条消息重试 Edit** 即可，文件未变则无需再 Read。
- **Edit 后勿同参 Read**：会 dedup stub。看新正文用 Grep 定位 → Read 不同 offset/limit，或 Grep 匹配行直接作下一次 Edit 的 `old_string`。

**按 Subagent 报告 Edit（强制）**：
- `old_string` **只能**来自 Read/Tail `tool_result`（去掉 `行号\t`），**禁止**用 CCA/KA 概括句、示例句、禁用模式描述当 `old_string`
- 报告给行号：`Grep` 该行附近独特词 → `Read offset/limit` → 复制 exact 片段
- 报告写「保留 / 无需修改」→ **跳过** Edit（避免 identical）
- `old_string not found on disk`：**不是** cache 失效；先 Grep，禁止同参反复 Read

修改优先级（从基础到表层）：
1. **世界观/战力文件**
2. **大纲**
3. **人物卡**
4. **细纲**
5. **正文章节**（优先改最近受影响的章节）

每章修改后追加细纲修订记录行：
| 日期/轮次 | 修订原因 | 修改范围 | 级联影响检查 |

## 改大纲流程

当作者要修改大纲（改卷方向/增删卷/调整章节分配）：

1. Read `knowledge/plot/大纲.md` 文件头注释 → 获取级联检查清单
2. ImpactAnalysis 定位受影响范围
3. 按检查清单逐项处理：
   - **已写章细纲**：Read 场景拆分，判断是否仍与大纲概要一致。不一致 → AskUserQuestion 确认是否重写
   - **未写章细纲**：直接 Edit 对齐新大纲，改完后 Fork PlanAuditor 重新审计
   - **伏笔追踪**：ForeshadowTracker 检查跨卷伏笔是否受影响
   - **因果链**：PlotGraph 检查跨卷因果边是否断裂
4. 全部修改完成后 → Fork PlanAuditor 审计受影响细纲

## 删章流程

不直接删除文件（无 rm 工具）：
1. 在伏笔追踪、因果链中追加"已废弃"标注
2. 大纲中截断或标记废弃章
3. 用 Write 空文件或 Edit 替换为废弃说明覆盖细纲/正文（或请作者在 IDE 中删除）
4. 更新 INDEX.md 进度段

## 本阶段完成后

1. 向用户汇报修订摘要：已修改文件清单与变更要点。
2. 若本轮 Edit/Write 了 `chapters/**`：**必须**在同一次 assistant 消息内并行 Fork 2 项 Subagent（KnowledgeAuditor + ChapterCraftAnalyzer），task 含受影响章节路径与改稿原因；按全部报告 Edit 修复。
3. 若仅改 knowledge/ 未改正文：说明知识库已同步；若改动可能影响已写章节，**必须** Fork KnowledgeAuditor（+ 必要时 ChapterCraftAnalyzer）审计最近相关章。
4. 审计与修复完成后，更新审计台账；改细纲/正文后将对应列标 `待复审` 或修复后标 `已通过`，向用户确认「修订完成」。

**下一步：** 如果修改了正文章节→已完成审计（KnowledgeAuditor + ChapterCraftAnalyzer），如需额外收尾核对→InvokeSkill(`post-chapter-checklist`)。如果修改了 knowledge/ 未改正文→说明已同步。如果修改了大纲/细纲→可能需要 InvokeSkill(`chapter-writing`) 重写或续写受影响章。如果改稿不满意要重来→InvokeSkill(`chapter-writing`) 使用重写模式。
