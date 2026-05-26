---
name: chapter-writing
description: 章节写作工作流——写前准备（含续写时必读上一章）、反 AI 味约束、写后同步知识库。当用户要写章、续写、产出正文时使用。触发词："写第X章"、"续写"、"写正文"、"下一章"
when_to_use: 写章/续写/产出正文时使用。策划、改稿、纯审计任务可忽略本 Skill。
skill_kind: workflow
allowed-tools: Read, Write, Edit, Glob, Grep, Bash, CharacterSearch, PlotGraph, Tail, InvokeSkill, TodoWrite, Stats, ForkSubAgent
---

# 章节写作工作流

## 阶段说明

本 Skill 覆盖**写章阶段** SOP。写前读知识库 → 写中遵循约束 → 写后同步更新——全部由你按本 Skill 执行。

---

## 第一步：写前准备

以下标记 **「必做」** 的步骤不可跳过；**「按需」** 在条件满足时执行。

### 必做

1. **确认进度**：Read `knowledge/INDEX.md`，对照 TodoWrite 和 `chapters/` 文件数量，双重确认当前应写哪一章

2. **读大纲**：Read `knowledge/plot/大纲.md` → 确认本章在大纲中的承上启下位置

3. **读细纲**：Read `knowledge/plot/细纲/chapter-NNN-细纲.md` → 场景拆分、出场人物、伏笔清单、知识库更新清单。如细纲文件不存在，Read `knowledge/plot/细纲/_template.md` 获取模板后自行产出细纲再继续

4. **查人物**：CharacterSearch 查询细纲中所有出场人物。对每人用 Grep + Read 取各演变日志**最后行**（勿通读整份人物卡）：
   ```
   CharacterSearch("陆沉") → path
   Grep pattern="^\| Ch" search_root="knowledge/characters/陆沉.md"
   → 各演变日志最后匹配行号 L
   Read file_path="..." offset=L-1 limit=5
   ```
   关注：性格演变日志 → 当前性格；关系演变日志 → 当前关系；已知信息演变日志 → POV 信息边界；修为/功法演变日志（如有）→ 当前战力

5. **查称呼**：Read `knowledge/characters/_关系与称呼索引.md` → 所有出场人物的当前称呼，双向速查

6. **追溯因果**：PlotGraph 查询细纲中伏笔关键词的 backward 因果链路

7. **读上一章**（第 2 章起必做；第 1 章跳过）：

   **先判断是否需要读盘**：若上一章 **Tail/Read 的 tool_result 仍在当前对话中**且该文件此后无 Edit/Write，则直接分析上下文即可，**跳过 Tail**。若你对该章做过 Edit/Write，须 Tail 末段或 Read 改动 range **一次**再续写。长章（>200 行）**从不** full Read；与 session cache 无关。

   如需读取：
   ```
   Tail(file_path="chapters/chapter-{N-1:03d}.md", lines=100)
   ```
   查人物演变日志末行：仍用 CharacterSearch → Grep `^\| Ch` → Read offset/limit（**勿 Tail 人物卡整文件**）。
   例：写第 5 章 → 读 `chapters/chapter-004.md` 末 100 行。重点关注：
   - **衔接点**：上章结尾钩子、场景、悬念；本章开篇须自然续接，勿另起炉灶
   - **叙事视角**：上章末段从哪个角色的「眼睛」写、用什么人称（如第三人称限知）；续写勿无过渡切换视角或人称
   - **文风**：上章末段句式与高频词；本章勿重复

   若需读取但 `chapter-{N-1}` 不存在：Glob `chapters/chapter-*.md` 确认实际前章；缺章或章号跳跃时 AskUserQuestion。**不得**在不知道上章内容的情况下 Write。

### 按需

8. **题材 Skill**：如本章涉及流派特定规则（仙侠战力体系、科幻科技约束、末世生存规则等），InvokeSkill 加载对应题材 Skill 的完整正文

9. **多世界题材**：如作品涉及多世界（`knowledge/worlds/` 非空），Read 本章所在世界的 `INDEX.md` 及该世界出场人物的人物卡

10. **追踪文件**：以下文件如**存在**则 Read 最后行（或 Grep 命中段）——不存在则跳过，勿逐一确认是否存在：
    - `knowledge/shared-systems/战力系统.md`
    - `knowledge/shared-systems/场景追踪.md`
    - `knowledge/shared-systems/道具追踪.md`

---

## 第二步：写作中

### 硬性约束

- **称呼**：严格对照 `_关系与称呼索引.md` 末行（当前称呼）
- **POV 信息边界**：正文若跟随某角色视角叙事（POV 角色），只能写该角色**已知信息演变日志**末行所允许的内容——角色不知道的信息不可写入其视角段落（具体视角类型见 `AGENTS.md`）
- **战力/科技逻辑**：遵守对应追踪文件中的约束规则
- **场景/道具**：与场景追踪、道具追踪日志最后行一致
- **字数**：2000–3000 字/章，结尾留钩子

### 反 AI 味（正文硬性约束）

Write 本章时对照下表（与 `system.md` §6.3 一致）。**前五项写后可 Grep 快检**，后五项写作时自觉规避。

| 维度 | 典型表现 | 标准 / 改法 |
|------|----------|------------|
| **「然后」** | 动作链「他然后…然后…」堆叠 | 单章 **≤3 次**；改分句、逗号 |
| **「不是…而是…」** | 解释性对比句 | **全章禁用** |
| **破折号（——）** | 插入说明/转折 | **全章 ≤1 次** |
| **排比** | 首先/其次/一方面/另一方面；连续同结构句 | **禁止**清单式排比 |
| **环境描写** | 进场景逐物扫视，与冲突/情绪无关 | **2–3 个**相关细节即可；过渡不铺全景 |
| **句式** | 句长均匀、密度一致 | 长短句交替 |
| **情感** | 空贴抽象标签 | 动作、对话、生理反应、具体记忆 |
| **详略** | 说明书式平滑、过渡冗长 | 冲突细写、过渡略写；可留 1–2 处闲笔 |
| **套路** | 最安全接龙写法 | 按细纲但拒绝模板化 |
| **连贯** | 重复上章末段句式 | 接钩子开篇，不复制上章收束（写前已读上章 tail） |

---

## 第三步：写后自检

Write 正文完成后逐项确认：

- **上章衔接**（N≥2）：开篇是否承接上章钩子，叙事视角与人称是否连贯；脱节则 Edit 开篇
- **字数**：2000–3000 字；不足 2000 需扩充，超过 3000 考虑拆分或征求作者意见
- **称呼一致性**：抽查对话和叙述中的称呼，与 `_关系与称呼索引.md` 末行对照
- **伏笔密度**：本章回收伏笔 ≥3 个时，考虑是否过于密集、应分散到后续章节
- **角色出场**：检查是否有重要角色已连续 ≥5 章未出场，在汇报中提醒作者
- **反 AI 味快检**（Grep 前五项）：`然后` >3、`不是.*而是` 出现、`——` >1、`首先|其次|再次|一方面|另一方面` 出现；开篇/转场连续 3+ 句纯环境铺陈且无人物动作 → 须 Edit

---

## 第四步：写后更新知识库

以下文件如本章涉及变化则 **append**（只追加，不覆写已有行）。Edit 时 `old_string` = 表末行，`new_string` = 末行 + 新行。

| 文件 | 触发条件 | 操作 |
|------|----------|------|
| `knowledge/plot/细纲/chapter-NNN-细纲.md` | 每次 | 填写「实际完成」「知识库更新确认」「修订记录」 |
| `knowledge/shared-systems/时间线.md` | 本章有关键时间戳 | append 时间戳行 |
| `knowledge/shared-systems/场景追踪.md` | 场景状态变化 | append 变化行 |
| `knowledge/shared-systems/道具追踪.md` | 道具归属/状态变化 | append 变化行 |
| `knowledge/shared-systems/势力追踪.md` | 势力关系变化 | append 变化行 |
| 各出场人物卡 | 每次 | 出场记录 append；性格/关系/已知信息/修为如有变化逐表 append |
| `knowledge/characters/_关系与称呼索引.md` | 称呼或关系变化 | **双向**各 append 一行 |
| `knowledge/plot/伏笔追踪.md` | 伏笔推进/回收/新埋 | append 对应行 |
| `knowledge/plot/因果链.md` | 新增因果边 | append 边 |
| `knowledge/INDEX.md` | 每次 | 更新最后出场章列和当前进度段 |
| `knowledge/shared-systems/功法技能.md` | 新增/揭示功法 | append 百科行 |
| TodoWrite | 每次 | 本章标记 completed |

---

## 本阶段完成后

1. 确认正文已 Write 至 `chapters/chapter-NNN.md`，字数 2000–3000
2. 确认上表中有触发条件的文件已全部 append（勿覆写已有行）
3. InvokeSkill(`post-chapter-checklist`) 逐项核对同步完整性
4. **同一次 assistant 消息**内并行 Fork **2 项** Subagent（task 均含 `chapters/chapter-NNN.md`；ChapterCraftAnalyzer 含 POV/主角名）：
   - KnowledgeAuditor · ChapterCraftAnalyzer
5. 按全部 Subagent 报告 Edit 修复后，向作者汇报：本章摘要、钩子、出场人物、待确认项、审计结论

**禁止** 跳过步骤 4 或只跑部分 Subagent 即向作者说「本章完成」。**禁止** 仅用 ConsistencyCheck 代替 ForkSubAgent 做写后签收。
