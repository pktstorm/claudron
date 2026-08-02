import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ScanProgress } from "../types/scan";

/** Subscribe to scan progress. Returns an unlisten function. */
export function onScanProgress(cb: (p: ScanProgress) => void): Promise<UnlistenFn> {
  return listen<ScanProgress>("scan-progress", (e) => cb(e.payload));
}
