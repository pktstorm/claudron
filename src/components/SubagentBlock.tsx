import { useEffect, useState } from "react";
import { loadSubagent } from "../api/conversation";
import type { Turn } from "../types/conversation";
import { TurnBlock } from "./TurnBlock";

export function SubagentBlock({
  sessionId,
  agentId,
  onClose,
}: {
  sessionId: string;
  agentId: string;
  onClose: () => void;
}) {
  const [turns, setTurns] = useState<Turn[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    loadSubagent(sessionId, agentId)
      .then((c) => live && setTurns(c.turns))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [sessionId, agentId]);

  return (
    <section className="my-2 ml-6 border-l-2 border-l-purple-500/40 pl-3">
      <header className="flex items-center justify-between text-[11px] text-purple-300">
        <span>Subagent {agentId.slice(0, 8)}</span>
        <button type="button" onClick={onClose} className="hover:underline">
          hide
        </button>
      </header>
      {error && (
        <p role="alert" className="text-xs text-amber-400">
          Subagent transcript unavailable.
        </p>
      )}
      {!error && turns === null && <p className="text-xs text-neutral-500">Loading…</p>}
      {turns?.map((t, i) => (
        <TurnBlock key={`${t.uuid}-${i}`} turn={t} onExpandSubagent={() => {}} />
      ))}
    </section>
  );
}
