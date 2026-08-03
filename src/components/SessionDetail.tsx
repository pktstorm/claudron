import { useState } from "react";
import type { Annotation, Session } from "../types";
import { focusSession, resumeSession } from "../api/tauri";
import { StatusPicker } from "./StatusPicker";

export function SessionDetail({
  session,
  onAnnotationChange,
}: {
  session: Session | null;
  onAnnotationChange: (a: Annotation) => void;
}) {
  // Reset on session change comes from the `key` this is rendered with, not
  // from an effect. An effect that calls setState synchronously makes React
  // render, run the effect, set state, and render again -- and React's own
  // guidance for "reset all state when a prop changes" is a key, which does it
  // in one pass.
  const [actionError, setActionError] = useState<string | null>(null);

  if (!session) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-neutral-500">
        Select a session to see its details.
      </div>
    );
  }

  const a = session.annotation;
  const title = a.displayName ?? session.aiTitle ?? "Untitled session";
  const isRunning = session.liveness === "legacy" || session.liveness === "managed";

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-5">
      <header>
        <h1 className="text-lg font-semibold text-neutral-100">{title}</h1>
        <p className="mt-0.5 text-xs text-neutral-500">
          {session.projectLabel}
          {session.gitBranch && <span className="ml-2 text-neutral-400">{session.gitBranch}</span>}
        </p>
      </header>

      {actionError && (
        <div role="alert" className="rounded bg-amber-500/15 px-3 py-2 text-xs text-amber-300">
          {actionError}
        </div>
      )}

      <div className="flex gap-2">
        {isRunning && (
          <button
            type="button"
            onClick={() => {
              setActionError(null);
              void focusSession(session.cwd).catch((e) => setActionError(String(e)));
            }}
            className="rounded bg-sky-500/15 px-3 py-1.5 text-sm text-sky-300 hover:bg-sky-500/25"
          >
            Jump to terminal
          </button>
        )}
        <button
          type="button"
          onClick={() => {
            setActionError(null);
            void resumeSession(session.sessionId, session.cwd).catch((e) => setActionError(String(e)));
          }}
          className="rounded bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700"
        >
          Resume in new tab
        </button>
      </div>

      {isRunning && (
        <p className="text-[11px] text-neutral-500">
          A claude process is running in this directory — it may be a different session.
        </p>
      )}

      <section>
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Your status
        </h3>
        <p className="mb-1 text-[11px] text-neutral-600">
          Set a label for yourself — this is not detected.
        </p>
        <StatusPicker value={a.status} onChange={(status) => onAnnotationChange({ ...a, status })} />
      </section>

      <section className="flex min-h-0 flex-1 flex-col">
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Notes
        </h3>
        <textarea
          value={a.notes}
          onChange={(e) => onAnnotationChange({ ...a, notes: e.target.value })}
          placeholder="Notes for this session…"
          className="min-h-[8rem] flex-1 resize-none rounded bg-neutral-800 p-2 text-sm text-neutral-100 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-sky-500"
        />
      </section>

      <section>
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Last prompt
        </h3>
        <p className="whitespace-pre-wrap break-words rounded bg-neutral-800/50 p-2 text-xs text-neutral-300">
          {session.lastPrompt ?? "—"}
        </p>
      </section>

      <footer className="text-[11px] text-neutral-600">
        <div className="break-all">{session.cwd}</div>
        <div>
          {session.sessionId}
          {session.version && ` · v${session.version}`}
        </div>
      </footer>
    </div>
  );
}
