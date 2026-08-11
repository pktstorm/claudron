import { useEffect, useState } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { listSessions } from "./api/tauri";
import { Rail, type ViewId } from "./components/Rail";
import { SessionsView } from "./components/SessionsView";
import { DashboardView } from "./components/DashboardView";
import { dismissSplash, setSplashProgress } from "./splash";
import { onScanProgress } from "./api/scan";
import { scanFraction } from "./types/scan";

/**
 * Dismiss the launch splash once the first scan resolves -- or fails.
 *
 * Lives HERE, above the view switch, not in `SessionsView`. Dismissal is driven
 * by the sessions query settling, and if that lived with the session view then
 * opening on any other view would leave the splash up forever -- a
 * `position: fixed; inset: 0` element that, per splash.ts, swallows every click
 * even at zero opacity. The app would look frozen at launch with no error shown.
 *
 * Running the query here costs no extra request: `SessionsView` uses the same
 * `["sessions"]` key, and react-query serves both from one fetch.
 *
 * Driven by the query rather than a timer because scan duration depends on how
 * many transcripts exist -- 11.6s measured cold, ~19ms warm -- so any fixed
 * delay is wrong on one machine or the other. Dismissing on `error` too matters:
 * a failed scan must not leave the splash covering the error underneath.
 */
function useSplash() {
  const { isLoading, error } = useQuery({
    queryKey: ["sessions"],
    queryFn: listSessions,
    refetchInterval: 10000,
  });

  const settled = !isLoading || !!error;
  useEffect(() => {
    if (settled) dismissSplash();
  }, [settled]);

  // The listener is registered once and torn down on unmount;
  // `setSplashProgress` no-ops after the splash is gone, so a late event from
  // an in-flight scan cannot resurrect it.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onScanProgress((p) => setSplashProgress(scanFraction(p)))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      // Subscribing can fail -- outside Tauri (tests, a browser dev server)
      // there is no event bridge at all. A progress bar is decoration; failing
      // to subscribe must never take the app down or surface as an unhandled
      // rejection. The splash still dismisses, because that is driven by the
      // query, not by this.
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}

function Shell() {
  // `useState`, not a router: there are two views. A dependency that owns
  // navigation is hard to remove later, and nothing here needs URLs or history.
  const [view, setView] = useState<ViewId>("sessions");
  useSplash();

  return (
    <div className="flex h-screen bg-neutral-900 text-neutral-100">
      <Rail active={view} onSelect={setView} />
      {view === "sessions" ? <SessionsView /> : <DashboardView />}
    </div>
  );
}

export default function App() {
  // Per mount, not module scope: a module-level client shares one cache across
  // every render in a test run, so a render could show the previous test's
  // sessions before its own fixture resolved. `useState` with a lazy
  // initializer -- not `useMemo`, which React is permitted to discard and
  // recompute, silently resetting the cache mid-session.
  const [client] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  );
  return (
    <QueryClientProvider client={client}>
      <Shell />
    </QueryClientProvider>
  );
}
