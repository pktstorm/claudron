import type { Session } from "../types";
import { SessionRow } from "./SessionRow";

export function SessionList({
  sessions,
  selectedId,
  onSelect,
  versionBaseline,
}: {
  sessions: Session[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  versionBaseline: string | null;
}) {
  if (sessions.length === 0) {
    return <div className="p-4 text-sm text-neutral-500">No sessions match the current filters.</div>;
  }

  // Live sessions lift out of their project groups into a single pinned group.
  // Grouping by project otherwise buries a live session under whole idle
  // groups, because a group's position is set by its first-seen member --
  // which defeated the backend's live-first sort entirely.
  const isLive = (s: Session) => s.liveness === "legacy" || s.liveness === "managed";
  const live = sessions.filter(isLive);
  const rest = sessions.filter((s) => !isLive(s));

  const groups = new Map<string, Session[]>();
  if (live.length) groups.set("Active", live);
  for (const s of rest) {
    const list = groups.get(s.projectLabel) ?? [];
    list.push(s);
    groups.set(s.projectLabel, list);
  }

  return (
    <div className="divide-y divide-neutral-800">
      {[...groups.entries()].map(([label, items]) => (
        <section key={label}>
          <h2 className="sticky top-0 bg-neutral-900/95 px-3 py-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
            {label}
          </h2>
          {items.map((s) => (
            <SessionRow
              key={s.sessionId}
              session={s}
              selected={s.sessionId === selectedId}
              onSelect={onSelect}
              versionBaseline={versionBaseline}
            />
          ))}
        </section>
      ))}
    </div>
  );
}
