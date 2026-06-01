# 作品记忆索引

本文件记录跨会话的作者偏好与已确认决策。Agent 可在对话中通过 Write/Edit 维护 `memory/` 下条目。

## 索引

| 主题 | 文件 | 说明 |
|------|------|------|
| 作者偏好 | [preferences.md](preferences.md) | 文风、禁忌、更新节奏 |
| 题材子类型 | [genre.md](genre.md) | 标签、对标作品、卖点 |
| 已确认决策 | [decisions.md](decisions.md) | 不可逆设定与 CP/剧情决定 |

## 维护指引

- 用户明确表达的偏好 → 写入 `memory/`
- 作品规范（POV、字数）→ 写在 `AGENTS.md`
- 压缩后 Memory 经 system `## Memory` 节在下次 compact 时刷新（非 `[上下文刷新]` user）
