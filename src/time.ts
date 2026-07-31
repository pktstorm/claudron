/// Render a unix-seconds timestamp as a short relative age ("3m", "2h", "5d").
export function relativeAge(unixSeconds: number, now = Date.now()): string {
  if (!unixSeconds) return "";
  const secs = Math.max(0, Math.floor(now / 1000) - unixSeconds);
  if (secs < 60) return "now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d`;
}
