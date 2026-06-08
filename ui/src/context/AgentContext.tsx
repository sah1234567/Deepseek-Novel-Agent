import { createContext, useContext, type ReactNode } from "react";
import { useAgent, type UseAgentReturn } from "../hooks/useAgent";
import type { AppStatus } from "../hooks/useAppStatus";

const AgentContext = createContext<UseAgentReturn | null>(null);

export function AgentProvider({
  onTurnComplete,
  children,
}: {
  onTurnComplete?: (prefetched?: AppStatus) => void;
  children: ReactNode;
}) {
  const agent = useAgent(onTurnComplete);
  return <AgentContext.Provider value={agent}>{children}</AgentContext.Provider>;
}

export function useAgentContext(): UseAgentReturn {
  const ctx = useContext(AgentContext);
  if (!ctx) throw new Error("useAgentContext must be used within AgentProvider");
  return ctx;
}
