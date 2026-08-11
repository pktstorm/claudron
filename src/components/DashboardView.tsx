/**
 * Placeholder for the stats dashboard (#15).
 *
 * A real component rather than an inline fragment, so it is the mount point
 * #15 fills in and so switching views is exercised by tests today. It reads no
 * data deliberately: wiring it to `dashboard_stats` would block the navigation
 * shell behind the extraction work in #71.
 */
export function DashboardView() {
  return (
    <main className="flex min-w-0 flex-1 flex-col items-center justify-center p-6">
      <p className="text-sm text-neutral-500">Nothing here yet.</p>
      <p className="mt-1 text-xs text-neutral-600">
        Session and token statistics will appear here.
      </p>
    </main>
  );
}
