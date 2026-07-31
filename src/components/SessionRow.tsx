import type { Liveness, Session } from "../types";
import { LIVENESS_LABEL, STATUS_LABEL } from "../labels";
import { relativeAge } from "../time";
import { isOlder } from "../version";

const LIVENESS_CLASS: Record<Liveness, string> = {
  managed: "bg-emerald-500/15 text-emerald-400",
  legacy: "bg-sky-500/15 text-sky-400",
  interrupted: "bg-amber-500/15 text-amber-400",
  idle: "bg-neutral-500/15 text-neutral-400",
};

export function SessionRow({
  session,
  selected,
  onSelect,
  versionBaseline,
}: {
  session: Session;
  selected: boolean;
  onSelect: (id: string) => void;
  versionBaseline: string | null;
}) {
  const title = session.annotation.displayName ?? session.aiTitle ?? "Untitled session";
  return (
    <button
      type="button"
      onClick={() => onSelect(session.sessionId)}
      className={`w-full border-l-2 px-3 py-2 text-left transition-colors ${
        selected
          ? "border-l-sky-400 bg-neutral-800"
          : "border-l-transparent hover:bg-neutral-800/50"
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium text-neutral-100">{title}</span>
        <span className="flex shrink-0 items-center gap-1">
          {session.version && (
            <span
              className={`shrink-0 text-[10px] ${
                versionBaseline &&
                (session.liveness === "legacy" || session.liveness === "managed") &&
                isOlder(session.version, versionBaseline)
                  ? "text-amber-400"
                  : "text-neutral-600"
              }`}
            >
              v{session.version}
            </span>
          )}
          <span className="shrink-0 text-[10px] text-neutral-500">{relativeAge(session.lastActivity)}</span>
          <span className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] ${LIVENESS_CLASS[session.liveness]}`}>
            {LIVENESS_LABEL[session.liveness]}
          </span>
        </span>
      </div>
      {session.gitBranch && (
        <div className="mt-0.5 text-xs text-neutral-500">
          <span className="truncate">{session.gitBranch}</span>
        </div>
      )}
      {session.annotation.status && (
        <span className="mt-1 inline-block rounded bg-purple-500/15 px-1.5 py-0.5 text-[10px] text-purple-300">
          {STATUS_LABEL[session.annotation.status]}
        </span>
      )}
    </button>
  );
}
