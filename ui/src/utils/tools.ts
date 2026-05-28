export function extractSearchRoot(input: unknown): string | null {
  if (!input || typeof input !== "object") return null;
  const obj = input as Record<string, unknown>;
  if (typeof obj.search_root === "string" && obj.search_root.trim()) return obj.search_root;
  // legacy SQLite messages
  if (typeof obj.path === "string" && obj.path.trim()) return obj.path;
  return null;
}

export function extractToolPath(input: unknown): string | null {
  if (!input || typeof input !== "object") return null;
  const obj = input as Record<string, unknown>;
  for (const key of ["file_path", "path", "target_file", "notebook_path"]) {
    const v = obj[key];
    if (typeof v === "string" && v.trim()) return v;
  }
  return null;
}

export function formatToolSummary(name: string, input: unknown): string {
  if (/^(Grep|Glob)/i.test(name)) {
    const root = extractSearchRoot(input);
    if (root) return name + ": " + root;
  }
  const path = extractToolPath(input);
  if (path && /^(Read|Write|Edit|Tail)/i.test(name)) {
    return name + ": " + path;
  }
  if (input && typeof input === "object") {
    const obj = input as Record<string, unknown>;
    if (name === "InvokeSkill" && typeof obj.skill_id === "string") {
      return obj.skill_id;
    }
    if (name === "CharacterSearch" && typeof obj.field === "string" && typeof obj.query === "string") {
      return obj.field + ": " + obj.query;
    }
    if (name === "PlotGraph" && typeof obj.direction === "string") {
      return obj.direction + ", depth " + (obj.depth ?? "?");
    }
    if (name === "ConsistencyCheck") return "9维扫描";
    if (name === "TodoWrite" && Array.isArray(obj.todos)) {
      return obj.todos.length + " 项";
    }
    if (name === "AskUserQuestion" && Array.isArray(obj.questions)) {
      return obj.questions.length + " 题";
    }
    if (name === "Bash" && typeof obj.command === "string") {
      return obj.command.length > 60 ? obj.command.slice(0, 60) + "…" : obj.command;
    }
  }
  return "";
}

/** Returns a human-readable one-liner describing what this tool call does. Never returns raw JSON. */
export function formatToolInput(name: string, input: unknown): string {
  if (!input || typeof input !== "object") return "";
  const obj = input as Record<string, unknown>;

  switch (name) {
    case "Read": {
      const fp = extractToolPath(obj) ?? "";
      const off = typeof obj.offset === "number" ? " L" + obj.offset : "";
      const lim = typeof obj.limit === "number" ? " (+" + obj.limit + ")" : "";
      return "读取 " + fp + off + lim;
    }
    case "Write": {
      const fp = extractToolPath(obj) ?? "";
      const len = typeof obj.content === "string" ? obj.content.length : 0;
      return "写入 " + fp + " (" + len + " 字符)";
    }
    case "Edit": {
      const fp = extractToolPath(obj) ?? "";
      const oldLen = typeof obj.old_string === "string" ? obj.old_string.length : 0;
      return "修改 " + fp + " (替换 " + oldLen + " 字符)";
    }
    case "Grep": {
      const pat = typeof obj.pattern === "string" ? obj.pattern : "?";
      const root = extractSearchRoot(obj);
      const pth = root ? " in " + root : "";
      return "搜索 \"" + pat + "\"" + pth;
    }
    case "Glob": {
      const pat =
        typeof obj.pattern === "string"
          ? obj.pattern
          : typeof obj.glob_pattern === "string"
            ? obj.glob_pattern
            : typeof obj.glob === "string"
              ? obj.glob
              : "?";
      const root = extractSearchRoot(obj);
      const pth = root ? " in " + root : "";
      return "匹配 " + pat + pth;
    }
    case "Bash": {
      const cmd = typeof obj.command === "string" ? obj.command : "?";
      return "执行: " + (cmd.length > 100 ? cmd.slice(0, 100) + "…" : cmd);
    }
    case "CharacterSearch": {
      const field = typeof obj.field === "string" ? obj.field : "?";
      const query = typeof obj.query === "string" ? obj.query : "?";
      return "搜索人物: " + field + " = \"" + query + "\"";
    }
    case "PlotGraph": {
      const dir = typeof obj.direction === "string" ? obj.direction : "?";
      const depth = typeof obj.depth === "number" ? obj.depth : "?";
      return "因果图遍历: " + dir + ", 深度 " + depth;
    }
    case "InvokeSkill": {
      const id = typeof obj.skill_id === "string" ? obj.skill_id
               : typeof obj.skillId === "string" ? obj.skillId : "?";
      return "加载 Skill: " + id;
    }
    case "TodoWrite": {
      const todos = Array.isArray(obj.todos) ? obj.todos : [];
      const names = todos.map((t: Record<string,unknown>) => t.content ?? "").filter(Boolean);
      return "更新待办: " + (names.length > 0 ? names.join(", ") : todos.length + " 项");
    }
    case "ConsistencyCheck": {
      return "执行一致性检查";
    }
    case "Tail": {
      const fp = extractToolPath(obj) ?? "?";
      const lines = typeof obj.lines === "number" ? obj.lines : 80;
      return "读末 " + lines + " 行: " + fp;
    }
    case "AskUserQuestion": {
      const qs = Array.isArray(obj.questions) ? obj.questions : [];
      const prompts = qs.map((q: Record<string,unknown>) => q.prompt ?? "").filter(Boolean);
      return "提问: " + (prompts.length > 0 ? prompts.join(" / ") : qs.length + " 题");
    }
    case "ImpactAnalysis":
      return "影响分析";
    case "KnowledgeDerive":
      return "知识库派生";
    case "WebSearch": {
      const q = typeof obj.query === "string" ? obj.query : "?";
      return "网页搜索: " + q;
    }
    case "Stats": {
      const ch = typeof obj.chapter === "string" ? obj.chapter : "";
      if (ch === "all") return "全局字数统计";
      if (ch) return `字数统计: 第${ch}章`;
      return "字数统计";
    }
    case "CharacterRotate":
      return "人物出场轮值检查";
    case "Corkboard":
      return "细纲场景卡片";
    case "ForeshadowTracker":
      return "伏笔追踪";
    case "PlotGrid":
      return "情节网格";
    default:
      return "";
  }
}

export const TODO_STATUS_CYCLE = ["pending", "in_progress", "completed"] as const;

export function nextTodoStatus(current: string): string {
  const idx = TODO_STATUS_CYCLE.indexOf(current as (typeof TODO_STATUS_CYCLE)[number]);
  if (idx === -1 || idx === TODO_STATUS_CYCLE.length - 1) return TODO_STATUS_CYCLE[0];
  return TODO_STATUS_CYCLE[idx + 1];
}
