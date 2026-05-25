import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ProjectFileEntry {
  path: string;
  isDir: boolean;
}

function normalizeProjectFileEntry(raw: ProjectFileEntry & { is_dir?: boolean }): ProjectFileEntry {
  return {
    path: raw.path,
    isDir: raw.isDir ?? raw.is_dir ?? false,
  };
}

export function useProjectFiles(enabled: boolean, activeWorkName?: string) {
  const [files, setFiles] = useState<ProjectFileEntry[]>([]);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (!enabled) {
      setFiles([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const list = await invoke<(ProjectFileEntry & { is_dir?: boolean })[]>("list_project_files");
      setFiles(list.map(normalizeProjectFileEntry));
      setError(null);
    } catch (e) {
      setFiles([]);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [enabled]);

  const openFile = useCallback(async (path: string, isDir: boolean) => {
    if (isDir) {
      setError(null);
      return;
    }
    setPreviewPath(path);
    try {
      const content = await invoke<string>("read_project_file", { path });
      setPreviewContent(content);
      setError(null);
    } catch (e) {
      setPreviewContent(null);
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, activeWorkName]);

  useEffect(() => {
    if (!enabled) return;
    const unlisteners: Promise<UnlistenFn>[] = [
      listen("session-resumed", () => {
        void refresh();
      }),
      listen("turn-complete", () => {
        void refresh();
      }),
    ];
    return () => {
      void Promise.all(unlisteners).then((fns) => fns.forEach((fn) => fn()));
    };
  }, [enabled, refresh]);

  const clearPreview = useCallback(() => {
    setPreviewPath(null);
    setPreviewContent(null);
  }, []);

  return {
    files,
    previewPath,
    previewContent,
    error,
    loading,
    refresh,
    openFile,
    clearPreview,
  };
}
