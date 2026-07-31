import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { loadConversation, pollConversation } from "../api/conversation";
import type { Turn, ToolResultUpdate } from "../types/conversation";
import { TurnBlock } from "./TurnBlock";
import { SubagentBlock } from "./SubagentBlock";

/// The open conversation polls far faster than the 10s session list: a tail
/// read is a few KB, and a slower cadence makes a live session feel dead.
const POLL_MS = 1000;

/// Append new turns and patch late-arriving results into turns already held.
///
/// Exported for testing. The patch half is load-bearing: measured, 60% of real
/// tool calls outlast the 1s poll interval, so their result arrives in a poll
/// after the one that delivered the call. Appending alone would leave most
/// calls reading "no result" forever.
export function applyDelta(
  prev: Turn[],
  incoming: Turn[],
  updates: ToolResultUpdate[],
): Turn[] {
  let next = updates.length === 0 ? prev : prev.map((t) => {
    const hit = t.blocks.some(
      (b) => b.kind === "tool" && updates.some((u) => u.toolUseId === b.call.id),
    );
    if (!hit) return t;
    return {
      ...t,
      blocks: t.blocks.map((b) => {
        if (b.kind !== "tool") return b;
        const u = updates.find((x) => x.toolUseId === b.call.id);
        if (!u) return b;
        return { ...b, call: { ...b.call, result: u.result, isError: u.isError } };
      }),
    };
  });
  return incoming.length ? [...next, ...incoming] : next;
}

export function ConversationPane({ sessionId }: { sessionId: string }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [offset, setOffset] = useState<number | null>(null);
  const [openAgent, setOpenAgent] = useState<string | null>(null);
  const [stuck, setStuck] = useState(true);
  const [pollError, setPollError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement | null>(null);

  const { data, error, isLoading } = useQuery({
    queryKey: ["conversation", sessionId],
    queryFn: () => loadConversation(sessionId),
  });

  // Reset all local state when the selected session changes.
  useEffect(() => {
    setTurns([]);
    setOffset(null);
    setOpenAgent(null);
    setStuck(true);
    setPollError(null);
  }, [sessionId]);

  useEffect(() => {
    if (!data) return;
    setTurns(data.turns);
    setOffset(data.offset);
  }, [data]);

  // Tail the transcript.
  useEffect(() => {
    if (offset === null) return;
    let live = true;
    // Never let polls stack. `setInterval` fires on a timer regardless of
    // whether the previous call finished, and two overlapping polls for one
    // session contend for the backend's pending-call state. The backend holds
    // a lock so nothing is lost, but a stacked queue of slow polls is wasted
    // work either way.
    let inFlight = false;
    // Stop after repeated failures rather than hammering a deleted transcript
    // once a second forever. A transient blip retries; a deleted file gives up.
    const MAX_CONSECUTIVE_FAILURES = 5;
    let failures = 0;
    const id = setInterval(() => {
      if (inFlight) return;
      inFlight = true;
      void pollConversation(sessionId, offset)
        .then((d) => {
          if (!live) return;
          failures = 0;
          if (d.reset) {
            setTurns(d.turns);
          } else if (d.turns.length || d.updates.length) {
            setTurns((prev) => applyDelta(prev, d.turns, d.updates));
          }
          if (d.offset !== offset) setOffset(d.offset);
        })
        .catch((e) => {
          if (!live) return;
          failures += 1;
          if (failures >= MAX_CONSECUTIVE_FAILURES) {
            setPollError(String(e));
            clearInterval(id);
          }
        })
        // Last in the chain: clearing the flag earlier would let the next tick
        // start while this one's handler is still running.
        .finally(() => {
          inFlight = false;
        });
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [sessionId, offset]);

  // Stick to the bottom only while the user is already there.
  useEffect(() => {
    if (stuck && scroller.current) {
      scroller.current.scrollTop = scroller.current.scrollHeight;
    }
  }, [turns, stuck]);

  function onScroll() {
    const el = scroller.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setStuck(atBottom);
  }

  if (error) {
    return (
      <div role="alert" className="p-4 text-sm text-amber-400">
        Could not load this conversation. {String(error)}
      </div>
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      {pollError && (
        <div role="alert" className="shrink-0 border-b border-amber-500/30 bg-amber-500/15 px-4 py-2 text-xs text-amber-300">
          Stopped following this conversation: {pollError}
        </div>
      )}
      <div ref={scroller} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
        {isLoading && <p className="p-4 text-sm text-neutral-500">Loading conversation…</p>}
        {!isLoading && turns.length === 0 && (
          <p className="p-4 text-sm text-neutral-500">Nothing to show for this session yet.</p>
        )}
        {turns.map((t, i) => (
          // Suffixed with the index because uuids are NOT unique in practice:
          // 7 of 1339 real transcripts repeat one across two lines, from
          // compaction/resume replay. Turns are only appended or wholly
          // replaced, never reordered, so the index is stable here.
          <div key={`${t.uuid}-${i}`}>
            <TurnBlock turn={t} onExpandSubagent={setOpenAgent} />
            {openAgent &&
              t.blocks.some((b) => b.kind === "tool" && b.call.agentId === openAgent) && (
                <SubagentBlock
                  sessionId={sessionId}
                  agentId={openAgent}
                  onClose={() => setOpenAgent(null)}
                />
              )}
          </div>
        ))}
      </div>
      {!stuck && (
        <button
          type="button"
          onClick={() => setStuck(true)}
          className="absolute bottom-3 right-4 rounded bg-sky-500/20 px-3 py-1 text-xs text-sky-300 hover:bg-sky-500/30"
        >
          Jump to latest
        </button>
      )}
    </div>
  );
}
