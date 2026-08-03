import { useState } from "react";
import type { Annotation, Session } from "../types";
import { SessionDetail } from "./SessionDetail";
import { GitTab } from "./GitTab";

export function DetailSlideOver({
  session,
  onAnnotationChange,
  onClose,
}: {
  session: Session | null;
  onAnnotationChange: (a: Annotation) => void;
  onClose: () => void;
}) {
  // Always opens on Overview: it is instant, whereas Git would greet the user
  // with a loading state.
  const [tab, setTab] = useState<"overview" | "git">("overview");

  // The slide-over stays mounted across session changes, so switching the
  // selected session must snap back to Overview rather than keep showing Git
  // content computed for the previous session's cwd.
  //
  // Adjusted DURING render rather than in an effect. A `key` cannot do it here
  // -- the caller keeps one instance across the change, which is the very
  // thing this handles -- and an effect would render the stale tab, set state,
  // and render again. React re-runs this component immediately without
  // committing the first pass, so the wrong tab is never painted.
  const [lastSessionId, setLastSessionId] = useState(session?.sessionId);
  if (session?.sessionId !== lastSessionId) {
    setLastSessionId(session?.sessionId);
    setTab("overview");
  }

  const tabClass = (t: string) =>
    `px-3 py-1.5 text-xs ${
      tab === t ? "border-b-2 border-b-sky-400 text-neutral-100" : "text-neutral-400"
    }`;

  return (
    <div className="absolute inset-0 z-10 flex">
      <button
        type="button"
        aria-label="Close details"
        onClick={onClose}
        className="flex-1 bg-black/40"
      />
      <aside
        data-testid="detail-slideover"
        className="flex w-96 flex-col border-l border-neutral-800 bg-neutral-900 shadow-xl"
      >
        <div className="flex shrink-0 border-b border-neutral-800">
          <button type="button" onClick={() => setTab("overview")} className={tabClass("overview")}>
            Overview
          </button>
          <button type="button" onClick={() => setTab("git")} className={tabClass("git")}>
            Git
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {tab === "overview" ? (
            <SessionDetail
              key={session?.sessionId ?? "none"}
              session={session}
              onAnnotationChange={onAnnotationChange}
            />
          ) : session ? (
            <GitTab
              cwd={session.cwd}
              isLive={session.liveness === "legacy" || session.liveness === "managed"}
            />
          ) : (
            <p className="p-4 text-sm text-neutral-500">Select a session.</p>
          )}
        </div>
      </aside>
    </div>
  );
}
