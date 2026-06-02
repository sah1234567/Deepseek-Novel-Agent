export const USER_INTERRUPT_MESSAGES = ["用户已中断", "Request was aborted"] as const;

export function isUserInitiatedInterruptMessage(message: string | undefined): boolean {
  if (!message) return false;
  return USER_INTERRUPT_MESSAGES.some((m) => message.includes(m));
}

export function shouldShowTurnError(payload: {
  phase?: string;
  message?: string;
  wasInterrupted?: boolean;
}): boolean {
  if (payload.wasInterrupted === true) return false;
  if (payload.phase !== "error") return false;
  return !isUserInitiatedInterruptMessage(payload.message);
}
