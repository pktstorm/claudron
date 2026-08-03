import { useEffect, useState } from "react";
import { gitLocal, gitRemote, removeWorktree } from "../api/git";
import type { GitLocal, GitRemote } from "../types/git";

const CHECK_CLASS: Record<string, string> = {
  passing: "text-emerald-400",
  failing: "text-red-400",
  pending: "text-amber-400",
  none: "text-neutral-500",
};

export function GitTab({ cwd, isLive }: { cwd: string; isLive: boolean }) {
  const [local, setLocal] = useState<GitLocal | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [remote, setRemote] = useState<GitRemote | null>(null);
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const [removed, setRemoved] = useState(false);
  const [nonce, setNonce] = useState(0);

  // Local first -- it is ~115ms against gh's ~566ms, so it must never wait.
  //
  // The clear-then-fetch pair is a genuine effect: it reacts to a cwd change or
  // a refresh by starting network work, which is exactly what effects are for.
  // Clearing synchronously is deliberate -- the alternative is rendering the
  // PREVIOUS directory's branch and dirty count under the new cwd until the
  // fetch resolves, which is worse than a loading state.
  useEffect(() => {
    let live = true;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLocal(null);
    setLocalError(null);
    gitLocal(cwd)
      .then((g) => {
        if (!live) return;
        setLocal(g);
        // Fresh data may no longer match what the open confirmation described
        // (path, branch, merge status) -- close it rather than leave a stale
        // confirmation clickable against premises that have changed.
        setConfirming(false);
      })
      .catch((e) => live && setLocalError(String(e)));
    return () => {
      live = false;
    };
  }, [cwd, nonce]);

  // A live process starting in this directory invalidates an open confirmation
  // the same way stale data would -- leaving "Confirm" clickable against a
  // guard that would now refuse.
  //
  // Derived during render rather than in an effect: whether the confirmation
  // shows is a function of state AND isLive, so there is nothing to
  // synchronise. An effect here would paint one frame with the confirmation
  // still open over a session that just went live.
  const showConfirm = confirming && !isLive;

  useEffect(() => {
    if (!local?.branch) return;
    let live = true;
    // Same reasoning as the local fetch above: clearing synchronously avoids
    // showing the previous branch's pull request under a new branch.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setRemote(null);
    setRemoteError(null);
    gitRemote(cwd, local.branch)
      .then((r) => live && setRemote(r))
      .catch((e) => live && setRemoteError(String(e)));
    return () => {
      live = false;
    };
  }, [cwd, local?.branch, nonce]);

  if (localError) {
    return (
      <div role="alert" className="p-4 text-sm text-amber-400">
        {localError}
      </div>
    );
  }
  if (!local) {
    return <p className="p-4 text-sm text-neutral-500">Loading git state…</p>;
  }

  const wt = local.worktree;
  const blockedReason = isLive
    ? "This session is running here — stop it before removing the worktree."
    : local.dirtyCount > 0
      ? `${local.dirtyCount} uncommitted change(s) — commit or discard them first.`
      : null;

  const mergeLabel =
    local.mergedIntoDefault === true
      ? "merged"
      : local.mergedIntoDefault === false
        ? "not merged"
        : "merge status unknown";

  return (
    <div className="flex flex-col gap-4 overflow-y-auto p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Git
        </h3>
        <button
          type="button"
          onClick={() => setNonce((n) => n + 1)}
          className="rounded bg-neutral-800 px-2 py-1 text-xs text-neutral-300 hover:bg-neutral-700"
        >
          Refresh
        </button>
      </div>

      <section>
        <p className="text-sm text-neutral-200">{local.branch ?? "detached HEAD"}</p>
        <p className="text-xs text-neutral-500">
          {local.dirtyCount === 0 ? "clean" : `${local.dirtyCount} changed`}
        </p>
      </section>

      <section>
        <p className="text-xs text-neutral-400">
          {local.ahead === null || local.behind === null
            ? "no upstream"
            : `${local.ahead} ahead, ${local.behind} behind upstream`}
        </p>
        <p className="text-[11px] text-neutral-600">
          Compared against your last fetch — Claudron does not fetch for you.
        </p>
      </section>

      {wt.isWorktree && !removed && (
        <section>
          <p className="text-xs text-neutral-400">Worktree of {wt.repoRoot}</p>
          {!showConfirm && (
            <p className="break-all text-[11px] text-neutral-600">{wt.path}</p>
          )}

          {blockedReason && (
            <p className="mt-1 text-[11px] text-amber-400">{blockedReason}</p>
          )}

          {!showConfirm ? (
            <button
              type="button"
              disabled={blockedReason !== null}
              onClick={() => setConfirming(true)}
              className="mt-2 rounded bg-red-500/15 px-3 py-1.5 text-xs text-red-300 hover:bg-red-500/25 disabled:cursor-not-allowed disabled:opacity-40"
            >
              Remove worktree
            </button>
          ) : (
            <div className="mt-2 rounded border border-red-500/30 bg-red-500/10 p-2">
              <p className="text-xs text-neutral-200">Remove this worktree?</p>
              <p className="mt-1 break-all text-[11px] text-neutral-400">{wt.path}</p>
              <p className="text-[11px] text-neutral-400">
                Branch <span className="text-neutral-300">{local.branch}</span> — {mergeLabel}
              </p>
              <p className="mt-1 text-[11px] text-neutral-500">
                The branch is kept. This session will no longer be resumable in this directory
                afterwards.
              </p>
              <div className="mt-2 flex gap-2">
                <button
                  type="button"
                  onClick={() => {
                    setRemoveError(null);
                    removeWorktree(wt.path)
                      .then(() => {
                        setRemoved(true);
                        setConfirming(false);
                      })
                      .catch((e) => setRemoveError(String(e)));
                  }}
                  className="rounded bg-red-500/20 px-3 py-1 text-xs text-red-300 hover:bg-red-500/30"
                >
                  Confirm remove
                </button>
                <button
                  type="button"
                  onClick={() => setConfirming(false)}
                  className="rounded bg-neutral-800 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-700"
                >
                  Cancel
                </button>
              </div>
              {removeError && (
                <p role="alert" className="mt-2 text-[11px] text-amber-400">
                  {removeError}
                </p>
              )}
            </div>
          )}
        </section>
      )}

      {removed && (
        <p className="text-xs text-emerald-400">Worktree removed. The branch was kept.</p>
      )}

      <section>
        {remoteError ? (
          <p className="text-xs text-amber-400">{remoteError}</p>
        ) : !local.branch ? (
          <p className="text-xs text-neutral-500">No branch — cannot look up a pull request.</p>
        ) : !remote ? (
          <p className="text-xs text-neutral-500">Loading pull request…</p>
        ) : remote.pullRequest ? (
          <>
            <p className="text-sm text-neutral-200">
              <span>#{remote.pullRequest.number}</span> <span>{remote.pullRequest.title}</span>
            </p>
            <p className={`text-xs ${CHECK_CLASS[remote.pullRequest.checks.state]}`}>
              {remote.pullRequest.state} · {remote.pullRequest.checks.passing} passing
              {remote.pullRequest.checks.failing > 0 &&
                `, ${remote.pullRequest.checks.failing} failing`}
              {remote.pullRequest.checks.pending > 0 &&
                `, ${remote.pullRequest.checks.pending} pending`}
            </p>
          </>
        ) : (
          <p className="text-xs text-neutral-500">No open pull request for this branch.</p>
        )}
      </section>
    </div>
  );
}
