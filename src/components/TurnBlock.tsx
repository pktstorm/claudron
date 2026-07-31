import type { Turn } from "../types/conversation";
import { ToolCallBlock } from "./ToolCallBlock";

export function TurnBlock({
  turn,
  onExpandSubagent,
}: {
  turn: Turn;
  onExpandSubagent: (agentId: string) => void;
}) {
  const isUser = turn.role === "user";
  return (
    <article
      className={`px-4 py-2 ${isUser ? "border-l-2 border-l-sky-500/40 bg-neutral-800/30" : ""}`}
    >
      {turn.blocks.map((b, i) =>
        b.kind === "text" ? (
          <p
            key={`text-${i}`}
            className="whitespace-pre-wrap break-words text-sm leading-relaxed text-neutral-200"
          >
            {b.text}
          </p>
        ) : (
          // Key by the call's own id, not the array index alone. A late result
          // patches this turn in place; an index-only key would tie identity to
          // position and silently collapse an expanded call if block order
          // ever changed. The index is still suffixed on because `call.id` can
          // be an empty string (parse.rs reads it with `.unwrap_or("")`), and
          // two malformed blocks in one turn would otherwise share a key.
          <ToolCallBlock key={`${b.call.id}-${i}`} call={b.call} onExpandSubagent={onExpandSubagent} />
        ),
      )}
      {turn.model && (
        <div className="mt-1 text-[10px] text-neutral-600">
          {turn.model}
          {turn.usage && ` · ${turn.usage.outputTokens.toLocaleString()} out`}
        </div>
      )}
    </article>
  );
}
