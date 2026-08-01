import { useEffect, useState } from "react";
import { hookStatus, installHooks, uninstallHooks } from "../api/hooks";
import type { HookPlan } from "../types/hooks";

/// Consent panel for installing Claudron's Claude Code hook.
///
/// This writes to `~/.claude/settings.json`, which belongs to Claude Code, not
/// to Claudron. So the exact JSON is shown BEFORE anything is written, and
/// removal is one click. Nothing here installs on launch or without a click.
export function HookSettings() {
  const [plan, setPlan] = useState<HookPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showSnippet, setShowSnippet] = useState(false);

  useEffect(() => {
    hookStatus()
      .then(setPlan)
      .catch((e) => setError(String(e)));
  }, []);

  async function run(action: () => Promise<HookPlan>) {
    setBusy(true);
    setError(null);
    try {
      setPlan(await action());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (error && !plan) {
    return (
      <section className="rounded border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-300">
        Could not read your Claude Code settings: {error}
      </section>
    );
  }

  if (!plan) {
    return <section className="p-3 text-xs text-neutral-500">Checking hook status…</section>;
  }

  const installed = plan.state === "installed";

  return (
    <section className="space-y-3 rounded border border-neutral-800 bg-neutral-900/60 p-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-medium text-neutral-200">Precise session tracking</h3>
          <p className="mt-1 text-xs leading-relaxed text-neutral-400">
            Without this, Claudron guesses which session is running by matching working
            directories, so two sessions in one repository look identical. A Claude Code hook
            makes it exact.
          </p>
        </div>
        <span
          className={`shrink-0 rounded px-2 py-0.5 text-[11px] ${
            installed
              ? "bg-emerald-500/15 text-emerald-300"
              : plan.state === "partial"
                ? "bg-amber-500/15 text-amber-300"
                : "bg-neutral-800 text-neutral-400"
          }`}
        >
          {installed ? "Installed" : plan.state === "partial" ? "Partly installed" : "Not installed"}
        </span>
      </div>

      <div className="space-y-1 text-[11px] text-neutral-500">
        <p>
          Adds {plan.events.length} entries to{" "}
          <code className="text-neutral-400">{plan.settingsPath}</code>
        </p>
        <p>
          Events: <span className="text-neutral-400">{plan.events.join(", ")}</span> — chosen
          because they cannot affect what Claude Code does. The hook only records which process is
          running which session, and always exits successfully.
        </p>
      </div>

      <button
        type="button"
        onClick={() => setShowSnippet((v) => !v)}
        className="text-[11px] text-sky-400 underline underline-offset-2 hover:text-sky-300"
      >
        {showSnippet ? "Hide" : "Show"} exactly what will be added
      </button>
      {showSnippet && (
        <pre className="overflow-x-auto rounded border border-neutral-800 bg-neutral-950 p-2 font-mono text-[10px] leading-relaxed text-neutral-300">
          {plan.settingsSnippet}
        </pre>
      )}

      {error && (
        <p role="alert" className="text-xs text-amber-400">
          {error}
        </p>
      )}

      <div className="flex gap-2">
        {installed ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(uninstallHooks)}
            className="rounded bg-neutral-800 px-2.5 py-1 text-xs text-neutral-200 hover:bg-neutral-700 disabled:opacity-50"
          >
            {busy ? "Removing…" : "Remove hook"}
          </button>
        ) : (
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(installHooks)}
            className="rounded bg-sky-600 px-2.5 py-1 text-xs text-white hover:bg-sky-500 disabled:opacity-50"
          >
            {busy ? "Installing…" : plan.state === "partial" ? "Repair hook" : "Install hook"}
          </button>
        )}
      </div>

      <p className="text-[11px] text-neutral-600">
        Removing it restores your settings and leaves everything else untouched. Claudron keeps
        working either way — without the hook it falls back to matching directories.
      </p>
    </section>
  );
}
