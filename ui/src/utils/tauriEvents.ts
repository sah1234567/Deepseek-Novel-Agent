import type { UnlistenFn } from "@tauri-apps/api/event";

/** Mount one or more Tauri event listeners; cleanup awaits all unlisten fns. */
export function mountTauriListeners(
  mounts: Array<() => Promise<UnlistenFn>>,
): () => void {
  const pending = mounts.map((mount) => mount());
  return () => {
    void Promise.all(pending).then((fns) => fns.forEach((fn) => fn()));
  };
}
