---
name: chapter-writing
description: 章节写作工作流——两层推进：先细纲（计划→登记→审计）再正文（写作→收尾→审计）。含重写模式与批量模式。触发词："写第X章"、"续写"、"写正文"、"下一章"、"重写第X章"、"连写"
when_to_use: 写章/续写/产出正文时使用。策划、改稿、纯审计任务可忽略本 Skill。
skill_kind: workflow
allowed-tools: Read, Write, Edit, Glob, Grep, Bash, CharacterSearch, PlotGraph, Tail, InvokeSkill, TodoWrite, Stats, ForkSubAgent, TrackingQuery, RelationQuery, Corkboard, ForeshadowTracker
---

# 章节写作工作流

写章分两层推进：**先细纲（计划层）→ 再正文（执行层）**。每层各有独立的知识库更新与审计步骤，追踪文件在细纲阶段一次性更新完毕，正文后仅做收尾与偏差修正。

### 模式判断（开始前先确认）

| 情况 | 使用模式 |
|------|---------|
| 本章正文不存在，正常写新章 | 完整两层流程 |
| `chapters/chapter-NNN.md` 已存在，作者不满意要求重写 | **重写模式**（跳过细纲阶段，正文阶段完整执行） |
| 作者要求连写多章（如 Ch8-10） | **批量模式**（细纲批量→一次审计；正文连写→一次审计） |

### 重写模式

1. 细纲已存在且 PlanAuditor 已通过 → **跳过细纲阶段**（细纲不变）
2. Read 细纲「修订记录」→ 了解之前的修改历史
3. Write 覆写正文 → 覆写细纲「写后记录」
4. 正文后审计不可跳过 → 正常 Fork KnowledgeAuditor + ChapterCraftAnalyzer
5. 如果重写后偏离细纲 → 按下方「偏离处理」执行
6. 细纲「修订记录」追加一行（原因=重写，范围=全章）

### 批量模式（多章连写）

当作者要求一次写多章（≤5 章）：
1. 细纲批量 Write → 批量更新追踪文件 → **一次性** Fork PlanAuditor（task 含所有章号）
2. 正文逐章 Write（每章单独 Stats + 写后记录）
3. 全部章写完后 → **一次性** Fork KnowledgeAuditor + ChapterCraftAnalyzer（task 含所有章号，提示按章分组输出）

批量模式下 PlanAuditor 可额外检查跨章伏笔节奏与人物弧线。

---

## 第一层：细纲阶段（策划 → 登记 → 审计）

「必做」不可跳过，「按需」在条件满足时执行。

### 写前必做

1. **确认进度**：Read `knowledge/INDEX.md`，对照 TodoWrite 和 `chapters/` 文件数量，双重确认当前应写哪一章
2. **读大纲**：Read `knowledge/plot/大纲.md` 当前卷概要 → 确认本章在卷级剧情中的位置
3. **门禁检查**：确认本卷大纲四要素（起点/冲突/转折/终点+钩子）齐全；不齐全 → 先补齐大纲再继续

### 编写细纲

4. Read `knowledge/plot/细纲/_template.md` 获取模板
5. Write `knowledge/plot/细纲/chapter-NNN-细纲.md`：
   - 场景拆分（整体 400 字左右，禁止写成小作文）
   - 出场人物清单
   - 伏笔清单（编号 + 状态 + 本章操作）
   - 知识库更新清单（带 `- [ ]` 勾选框）

### ★ 立即更新追踪文件（一次性登记计划值，禁止延至正文后）

以下文件如本章涉及变化则 **append**（只追加，不覆写已有行）。Edit 时 `old_string` = 表末行，`new_string` = 末行 + 新行。

| 文件 | 触发条件 | 操作 |
|------|----------|------|
| `knowledge/plot/细纲/chapter-NNN-细纲.md` | 每次 | 知识库更新清单逐条打勾 ✓ |
| `knowledge/plot/大纲.md` | 每次 | 章节索引表 append 本章行（Ch/标题/核心事件≤30字/POV/所在世界）。Grep `^\| Ch \|` 定位表头 → Grep 本卷表内末行 → Read ±2 行 → Edit append |
| `knowledge/plot/伏笔追踪.md` | 本章有伏笔操作 | append 每条操作行（章节号=本章，状态=计划中） |
| `knowledge/plot/因果链.md` | 本章有新增因果边 | append 事件边（章节号=本章） |
| 各出场人物卡 | 每次 | 出场记录/性格/关系/已知信息演变日志如有计划变化逐表 append（章节号=本章，可在内容中标注 [计划]） |
| `knowledge/characters/_关系与称呼索引.md` | 计划中称呼或关系变化 | **双向**各 append 一行 |
| `knowledge/shared-systems/场景追踪.md` | 计划中场景状态变化 | append 变化行 |
| `knowledge/shared-systems/道具追踪.md` | 计划中道具归属/状态变化 | append 变化行 |
| `knowledge/shared-systems/势力追踪.md` | 计划中势力关系变化 | append 变化行 |
| `knowledge/shared-systems/时间线.md` | 本章有关键时间戳 | append 时间戳行 |
| `knowledge/INDEX.md` | 每次 | 更新进度段 |

### ★ Fork PlanAuditor（不可跳过）

追踪文件更新完成后，Fork PlanAuditor：
```
ForkSubAgent(agent_type="PlanAuditor", task="审计细纲 chapter-NNN-细纲.md 的计划质量：大纲对齐、伏笔密度、因果闭合、人物轮换、字数分配、登记完整性")
```

按报告 Edit 细纲 → 更新 `knowledge/meta/audit-status.md`（细纲PA=已通过）。PlanAuditor 通过后，才可进入正文阶段。

---

## 第二层：正文阶段（写作 → 收尾 → 审计）

### 第一步：写前准备

**必做：**

1. **读细纲**：Read `knowledge/plot/细纲/chapter-NNN-细纲.md` → 场景拆分、出场人物、伏笔清单

2. **查人物**：CharacterSearch 查询细纲中所有出场人物。对每人：
   ```
   CharacterSearch("角色名") → path
   RelationQuery(character="角色名") → 当前关系与称呼
   Grep pattern="^\| Ch" → Read 各演变日志末行 ±2 行
   ```

3. **查当前状态（用专用工具，非全文 Read）**：
   - `TrackingQuery(file="power", operation="current")` — 战力当前状态
   - `TrackingQuery(file="scene", operation="current")` — 场景当前状态
   - `TrackingQuery(file="prop", operation="current")` — 道具当前状态
   - 如有势力线：`TrackingQuery(file="faction", operation="current")`
   - 如有关键时间节点：`TrackingQuery(file="timeline", operation="current")`

4. **读上一章**（第 2 章起必做；第 1 章跳过）：
   - 若上章 Tail/Read 的 tool_result 仍在对话中且此后无 Edit/Write → 直接使用
   - 否则：`Tail(file_path="chapters/chapter-{N-1:03d}.md", lines=100)`
   - 关注：衔接点（钩子/场景/悬念）、叙事视角连贯性、文风
   - 局部 Edit 优先；改稿读盘策略见 InvokeSkill(`revision`) 第二步「同文件连续改稿」（新区域须先 Read/Tail 进对话，**下一轮**再 Edit）

5. **追溯因果**：PlotGraph 查询细纲中伏笔关键词的 backward 因果链路

**按需：**

6. **题材 Skill**：如本章涉及流派特定规则，InvokeSkill 加载

### 第二步：写作中

**硬性约束：**
- **严格按细纲场景拆分逐场景写**，不跳场景
- **正文是连续叙事，不是结构化文档**：场景之间用空行、`※` 分隔符或自然叙事过渡衔接。**禁止**出现序号（一/二/三、1/2/3）、分节标题（如 `## 场景1`）、场景标注（如「场景一：」）——这些是细纲格式，不是正文格式
- **称呼**：以第一步 RelationQuery 返回的当前称呼为准
- **POV 信息边界**：正文跟随某角色视角叙事时，只能写该角色已知信息演变日志末行所允许的内容
- **字数**：2000–4000 字/章（各场景 ~字数合计应在此范围内）
- **结尾留钩子**

**偏离细纲时的处理（写作中发现更好的写法）：**

| 偏离程度 | 判定 | 处理 |
|---------|------|------|
| 微调 | 同一场景内对话/细节不同，核心动作和伏笔操作不变 | 继续写，正文后在细纲「知识库更新确认」标记「已变更」并备注 |
| 局部偏离 | 单个场景核心动作变化，但不影响后续章因果链 | 暂停→Edit 细纲对应场景→继续写→正文后在追踪文件追加状态更新行 |
| 重大偏离 | 场景增删、伏笔操作变化、影响后续章走向 | 暂停→AskUserQuestion 确认→更新细纲+追踪文件→重新 Fork PlanAuditor→再继续写 |

**禁止**不经确认的重大偏离——那会导致追踪文件与正文系统性不一致。

**反 AI 味（正文硬性约束——由 ChapterCraftAnalyzer 专项检查）：**

| 维度 | 标准 |
|------|------|
| **「然后」串联** | 单章作连接词 ≤3 次 |
| **「不是…(而)是…」** | 全章禁用（含「不是…而是…」「不是…是…」「并非…而是…」等变体） |
| **破折号（——）** | 全章 ≤1 次 |
| **排比** | 禁止清单式排比推进情节 |
| **环境描写** | 2–3 个相关细节即可；过渡不铺全景 |
| **结构化序号** | 禁止 `一、`/`1.`/`##`/`场景一：` 等分节格式——正文是连续叙事 |
| **Markdown 标记** | 禁止 `**加粗**` `*斜体*` 反引号 `~~删除线~~` `__下划线__` ——中文网文不用 Markdown |
| **句式** | 长短句交替 |
| **情感** | 用动作、对话、生理反应呈现，不空贴标签 |
| **详略** | 冲突细写、过渡略写 |
| **套路** | 按细纲写但拒绝模板化接龙 |
| **连贯** | 接钩子开篇，不复制上章收束 |

### 第三步：写后自检

Write 正文完成后逐项确认（**反AI味专项由 ChapterCraftAnalyzer 负责，此处仅检查以下项**）：

- **上章衔接**（N≥2）：开篇是否承接上章钩子，叙事视角与人称是否连贯；脱节则 Edit 开篇
- **字数**：`Stats(chapter="N")` 统计本章字数，确认 2000–4000。
  **★ 字数调整一律用 Edit 局部修改，禁止 Write 全文重写——重写引入新错误的代价远超局部修。**
  - **不足 2000 字**：Read 细纲场景拆分 → 定位偏短的场景 → Edit 扩充描写（补充对话/动作/心理/环境细节），不改动已写段落。扩充后 Stats 复验。
  - **超过 4000 字**：Grep 定位冗余 — `然后` >3、`——` >1、`首先\|其次\|一方面\|另一方面`、Markdown 标记 → 针对命中段逐段 Edit 删除或改写。精修后 Stats 复验；仍超标 → 征求作者意见是否拆分章节。
- **角色出场**：检查是否有重要角色已连续 ≥5 章未出场，在汇报中提醒作者

### 第四步：写后收尾（轻量，不全量更新追踪文件）

**追踪文件已在细纲阶段一次性更新完毕。正文后仅做以下收尾操作：**

1. 细纲「写后记录」填写：
   - 字数（Stats 结果）
   - 核心事件（本章实际发生的主要事件，≤30 字）
   - 钩子（本章结尾悬念）

2. 细纲「知识库更新确认」逐条标记：
   - 与计划一致 → 标记「已执行」
   - 执行方式与计划不同 → 标记「已变更」并备注新方式
   - 计划了但正文未涉及 → 标记「未执行」并说明原因
   - 本章不涉及 → 标记「无变化」

3. **★ 状态确认（"计划→实际"转换）：**
   对细纲「知识库更新确认」中标记为「已变更」或「未执行」的项：
   - 到对应追踪文件中，在计划条目的**下一行追加**状态更新行（不删除不覆写）
   - 伏笔追踪示例：计划行 `Ch8, F015, 埋设, 神秘玉佩, 计划中` → 追加 `Ch8, F015, 修正, 玉佩实际出现在街头偶遇场景, 已埋设`
   - 因果链、人物演变同理

4. **如有执行偏差**：
   - 在细纲「修订记录」中追加本轮修订行（日期 + 轮次 + 原因 + 范围）

5. **如有意外事件**（正文中发生了细纲未计划的重要事件）：
   - Edit append 到对应追踪文件
   - 修订记录中标记

---

## 本阶段完成后

1. 确认正文已 Write 至 `chapters/chapter-NNN.md`，`Stats(chapter="N")` 确认字数 2000–4000
2. 确认细纲「写后记录」和「知识库更新确认」已填
3. **★ 同一次 assistant 消息**内并行 Fork 2 项 Subagent：
   - **KnowledgeAuditor**：`审计 chapters/chapter-NNN.md 是否忠实执行细纲，收尾是否完整`
   - **ChapterCraftAnalyzer**：`分析 chapters/chapter-NNN.md：对话质量、叙事节奏、情感轨迹、设定一致性（称呼/POV边界/战力/场景道具）、反AI味指标。POV=XXX`
4. 按全部 Subagent 报告 Edit 修复后，更新审计台账（正文KA/文笔CCA=已通过），向作者汇报：本章摘要、钩子、出场人物、待确认项、审计结论

**按审计报告 Edit**：`old_string` 须从 Read/Tail tool_result 逐字复制（非报告引文）；报告给行号则 Grep→Read；写「保留」则跳过；`not found on disk` 时 Grep 锚点重定位，勿同参重读。

**禁止** 跳过步骤 3 或只跑部分 Subagent 即向作者说「本章完成」。
**禁止** 在正文后重复全量更新追踪文件——追溯文件应在细纲阶段一次性完成。

**下一步：** 审计修复完成 → 回复作者「本章完成」。如需继续写下一章 → 再次 Invoke 本 Skill。如需基于审计报告改稿 → InvokeSkill(`revision`)。如本章是独立写完（非通过本 Skill 的正文阶段）、只需收尾核对 → InvokeSkill(`post-chapter-checklist`)。
