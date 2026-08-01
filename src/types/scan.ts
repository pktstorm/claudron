/** Progress of a session-index scan, as emitted by the `scan-progress` event. */
export interface ScanProgress {
  filesDone: number;
  filesTotal: number;
  /** Progress is measured in bytes: file count would race to 99% then stall. */
  bytesDone: number;
  bytesTotal: number;
}

/** Fraction complete in 0..1. A zero total is complete, not a divide by zero. */
export function scanFraction(p: ScanProgress): number {
  if (p.bytesTotal === 0) return 1;
  return Math.min(1, Math.max(0, p.bytesDone / p.bytesTotal));
}
