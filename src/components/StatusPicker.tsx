import { STATUS_LABEL } from "../labels";
import type { ManualStatus } from "../types";

const OPTIONS: ManualStatus[] = ["blocked", "needsReview", "waitingOnMe", "background"];

export function StatusPicker({
  value,
  onChange,
}: {
  value: ManualStatus | null;
  onChange: (v: ManualStatus | null) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {OPTIONS.map((o) => (
        <button
          key={o}
          type="button"
          onClick={() => onChange(value === o ? null : o)}
          className={`rounded px-2 py-1 text-xs ${
            value === o
              ? "bg-purple-500/20 text-purple-300"
              : "bg-neutral-800 text-neutral-400 hover:text-neutral-200"
          }`}
        >
          {STATUS_LABEL[o]}
        </button>
      ))}
    </div>
  );
}
