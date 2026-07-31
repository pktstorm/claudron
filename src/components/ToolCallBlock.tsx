import { useState } from "react";
import type { ToolCall } from "../types/conversation";

/// Cap rendered output. A single `ls -la` result was already multi-KB in real
/// transcripts; sixty expanded would recreate the memory problem that made an
/// embedded terminal unattractive.
const MAX_RESULT_CHARS = 4000;

export function ToolCallBlock({
  call,
  onExpandSubagent,
}: {
  call: ToolCall;
  onExpandSubagent: (agentId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [full, setFull] = useState(false);
  const label = call.description ?? call.name;
  const result = call.result;
  const truncated = result !== null && result.length > MAX_RESULT_CHARS && !full;
  const shown = truncated ? result.slice(0, MAX_RESULT_CHARS) : result;

  return (
    <div className="my-1">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm text-neutral-400 hover:bg-neutral-800/60"
      >
        <span className="shrink-0 text-neutral-600">{open ? "▾" : "▸"}</span>
        <span className="truncate">{label}</span>
        {call.isError && (
          <span className="shrink-0 rounded bg-red-500/15 px-1.5 text-[10px] text-red-400">
            failed
          </span>
        )}
      </button>

      {open && (
        <div className="mt-1 pl-6">
          {shown === null ? (
            <p className="text-xs italic text-neutral-500">No result — the session ended before this finished.</p>
          ) : (
            <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded bg-neutral-900/80 p-2 text-xs text-neutral-300">
              {shown}
            </pre>
          )}
          {truncated && (
            <button
              type="button"
              onClick={() => setFull(true)}
              className="mt-1 text-[11px] text-sky-400 hover:underline"
            >
              Show full output ({result!.length.toLocaleString()} chars)
            </button>
          )}
        </div>
      )}
      {call.agentId && (
        <button
          type="button"
          onClick={() => onExpandSubagent(call.agentId!)}
          className="ml-6 mt-1 block text-[11px] text-sky-400 hover:underline"
        >
          Show subagent conversation
        </button>
      )}
    </div>
  );
}
