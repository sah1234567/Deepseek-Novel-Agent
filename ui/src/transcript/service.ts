import { invoke } from "@tauri-apps/api/core";
import { IPC_COMMANDS } from "../ipc/commands";
import type { ApiUiMessage } from "../utils/messages";

/** `sessionId: null` resolves to the current engine session (same as other IPC). */
export type SessionIdArg = string | null;

export type UiTurnBounds = {
  minTurn: number;
  maxTurn: number;
};

export type UiTurnBundle = {
  turnNumber: number;
  messages: ApiUiMessage[];
};

export type ArchiveEpochBounds = {
  epoch: number;
  bounds: UiTurnBounds;
};

export type SessionTranscriptLayout = {
  hasContextRefresh: boolean;
  active: UiTurnBounds;
  archives: ArchiveEpochBounds[];
};

export function fetchTranscriptLayout(sessionId: SessionIdArg): Promise<SessionTranscriptLayout> {
  return invoke<SessionTranscriptLayout>(IPC_COMMANDS.getSessionTranscriptLayout, { sessionId });
}

export function fetchActiveTurns(
  sessionId: SessionIdArg,
  fromTurn: number,
  toTurn: number,
): Promise<UiTurnBundle[]> {
  return invoke<UiTurnBundle[]>(IPC_COMMANDS.getSessionMessageTurns, {
    sessionId,
    fromTurn,
    toTurn,
  });
}

export function fetchArchiveTurns(
  sessionId: SessionIdArg,
  epoch: number,
  fromTurn: number,
  toTurn: number,
): Promise<UiTurnBundle[]> {
  return invoke<UiTurnBundle[]>(IPC_COMMANDS.getSessionArchiveTurns, {
    sessionId,
    epoch,
    fromTurn,
    toTurn,
  });
}
