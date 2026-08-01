import { useEffect, useMemo, useRef, useState } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { listSessions, setAnnotation } from "./api/tauri";
import { applyFilters, useFilters } from "./store/filters";
import { FilterBar } from "./components/FilterBar";
import { SessionList } from "./components/SessionList";
import { ConversationPane } from "./components/ConversationPane";
import { DetailSlideOver } from "./components/DetailSlideOver";
import type { Annotation } from "./types";

const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

function Shell() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const filters = useFilters();
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const { data, error, isLoading } = useQuery({
    queryKey: ["sessions"],
    queryFn: listSessions,
    // 10s, not 3s: a cold index scan of the real tree measured ~10s. With the
    // mtime cache a warm poll is far cheaper, but polling faster than a cold
    // scan risks overlapping requests on first launch.
    refetchInterval: 10000,
  });

  // Memoized because `?? []` allocates a fresh array whenever `data` is
  // undefined, which would change the identity `visible` depends on and re-run
  // applyFilters over every session on each render.
  const sessions = useMemo(() => data?.sessions ?? [], [data]);
  const versionBaseline = data?.versionBaseline ?? null;
  const visible = useMemo(() => applyFilters(sessions, filters), [sessions, filters]);
  const selected = sessions.find((s) => s.sessionId === selectedId) ?? null;

  // Debounce annotation writes so typing does not hit the disk on every key.
  const [draft, setDraft] = useState<Annotation | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [showDetails, setShowDetails] = useState(false);
  const shown = draft && selected ? { ...selected, annotation: draft } : selected;

  useEffect(() => {
    setDraft(null);
    setShowDetails(false);
  }, [selectedId]);

  // A pending debounced save must not fire after the component (or the
  // selected session) is gone.
  useEffect(() => {
    return () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, []);

  function onAnnotationChange(a: Annotation) {
    if (!selected) return;
    setDraft(a);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      // `draft` already shadows the polled value in the UI, and a full
      // ps+lsof discovery refetch on every debounced keystroke pause is
      // wasteful — the next scheduled poll will pick up the saved state.
      setSaveError(null);
      setAnnotation(selected.sessionId, a).catch((e) => setSaveError(String(e)));
    }, 500);
  }

  return (
    <div className="flex h-screen bg-neutral-900 text-neutral-100">
      <aside className="flex w-80 shrink-0 flex-col border-r border-neutral-800">
        <FilterBar />
        <div className="px-3 py-1 text-[11px] text-neutral-500">
          {isLoading ? "Loading…" : `${visible.length} sessions`}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {error ? (
            <div className="p-4 text-sm text-amber-400">Could not load sessions.</div>
          ) : (
            <SessionList
              sessions={visible}
              selectedId={selectedId}
              onSelect={setSelectedId}
              versionBaseline={versionBaseline}
            />
          )}
        </div>
      </aside>
      <main className="relative flex min-w-0 flex-1 flex-col">
        {saveError && (
          <div role="alert" className="border-b border-neutral-800 bg-amber-500/15 px-3 py-2 text-xs text-amber-300">
            Note not saved: {saveError}
          </div>
        )}
        {selected ? (
          <>
            <header className="flex shrink-0 items-center justify-between border-b border-neutral-800 px-4 py-2">
              <h2 className="truncate text-sm font-medium text-neutral-200">
                {shown?.annotation.displayName ?? selected.aiTitle ?? "Untitled session"}
              </h2>
              <button
                type="button"
                onClick={() => setShowDetails(true)}
                className="shrink-0 rounded bg-neutral-800 px-2 py-1 text-xs text-neutral-300 hover:bg-neutral-700"
              >
                Details &amp; notes
              </button>
            </header>
            {/* min-h-0 is required: without it a flex child refuses to shrink
                below its content and the pane's own scrolling never engages. */}
            <div data-testid="conversation-pane" className="min-h-0 flex-1">
              <ConversationPane sessionId={selected.sessionId} />
            </div>
          </>
        ) : (
          <div className="flex h-full items-center justify-center p-6 text-sm text-neutral-500">
            Select a session to see its conversation.
          </div>
        )}
        {showDetails && (
          <DetailSlideOver
            session={shown}
            onAnnotationChange={onAnnotationChange}
            onClose={() => setShowDetails(false)}
          />
        )}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={client}>
      <Shell />
    </QueryClientProvider>
  );
}
