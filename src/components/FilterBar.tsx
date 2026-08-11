import type { Liveness, ManualStatus } from "../types";
import { useFilters } from "../store/filters";
import { LIVENESS_LABEL, STATUS_LABEL } from "../labels";
import { Toggle } from "./ui/toggle";

const LIVENESS: Liveness[] = ["legacy", "interrupted", "idle"];
const STATUSES: ManualStatus[] = ["blocked", "needsReview", "waitingOnMe", "background"];

export function FilterBar() {
  const { search, liveness, status, setSearch, setLiveness, setStatus } = useFilters();
  return (
    <div className="space-y-2 border-b border-neutral-800 p-3">
      <input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search sessions, notes, branches…"
        aria-label="Search sessions"
        className="w-full rounded bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-sky-500"
      />
      <div className="flex flex-wrap gap-1">
        {LIVENESS.map((l) => (
          <Toggle
            key={l}
            size="sm"
            pressed={liveness === l}
            onPressedChange={(on) => setLiveness(on ? l : null)}
            className={liveness === l ? "bg-sky-500/20 text-sky-300" : "text-neutral-400"}
          >
            {LIVENESS_LABEL[l]}
          </Toggle>
        ))}
        {STATUSES.map((s) => (
          <Toggle
            key={s}
            size="sm"
            pressed={status === s}
            onPressedChange={(on) => setStatus(on ? s : null)}
            className={status === s ? "bg-purple-500/20 text-purple-300" : "text-neutral-400"}
          >
            {STATUS_LABEL[s]}
          </Toggle>
        ))}
      </div>
    </div>
  );
}
