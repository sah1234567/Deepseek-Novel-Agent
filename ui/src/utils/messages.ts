import type { ContentBlock, UIMessage } from "../hooks/useAgent";

export interface ApiUiMessage {
  id: string;
  role: string;
  contentBlocks: ContentBlock[];
  toolName?: string;
  forkRunId?: string;
  messageKind?: string;
}

export function apiMessagesToUi(messages: ApiUiMessage[]): UIMessage[] {
  return messages
    .filter(
      (m) =>
        m.role === "user" ||
        m.role === "assistant" ||
        m.role === "tool" ||
        m.role === "subAgentReport",
    )
    .map((m) => ({
      id: m.id,
      role: m.role as UIMessage["role"],
      contentBlocks: m.contentBlocks,
      toolName: m.toolName,
      forkRunId: m.forkRunId,
      messageKind: m.messageKind,
    }));
}
