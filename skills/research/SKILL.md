---
name: research
description: 自定义只读调研（Graph Research 节点 / 原 GeneralPurpose 调研映射）。Invoke 后在节点内执行 WebSearch/Read 并写报告正文。
---

# research — 调研 Skill

Graph-Primary 下 Research 节点或作者临时调研时 **InvokeSkill** 加载本手册。

- **只读分析**：结论写在 assistant 返回正文；落盘由节点 Agent Write/Edit
- 优先 `WebSearch`（角度见 tool schema）+ `Read` / `Grep`；禁止 Write/Edit 正典（调研笔记可经节点可写集）
- 报告末尾含 **`## 接下来（主 Agent 必读）`** 建议
- 需要独立上下文隔离时可选 `ForkSubAgent(GeneralPurpose)`，但 Graph 主路径是本 Skill

完成后 `GraphSubmitForApproval` 或继续 Edit 调研产出。
