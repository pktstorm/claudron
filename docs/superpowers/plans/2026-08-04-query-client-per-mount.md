# QueryClient Per Mount — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `App.tsx`'s `QueryClient` from module scope into the component, so each `render(<App />)` starts with an empty cache and tests cannot leak query data into one another.

**Architecture:** One-line change in `src/App.tsx` using a `useState` lazy initializer, plus the test that proves it and the documentation and comments that currently describe the old constraint. Production behaviour is unchanged — `main.tsx` mounts `App` exactly once.

**Tech Stack:** React 19, TanStack Query v5, Vitest 4, Testing Library, TypeScript.

**Spec:** `docs/superpowers/specs/2026-08-04-query-client-per-mount-design.md`
**Issue:** [#7](https://github.com/pktstorm/claudron/issues/7)

## Global Constraints

- **Nullable values are `T | null`, never `T?`.** Not exercised by this change, but do not introduce optional properties.
- **Use `??` for fallbacks, never `||`.** Enforced by `@typescript-eslint/prefer-nullish-coalescing` as an error.
- **There is no vitest `setupFiles`.** `@testing-library/jest-dom` matchers are NOT registered. Use `toBeDefined()`, `toBeNull()`, `toHaveLength()`, `toBe()`, `toEqual()`. `toBeInTheDocument()` will fail confusingly.
- **Docs are part of the change.** `CLAUDE.md` updates land in the **same commit** as the code, not a follow-up.
- **Do not weaken or delete a test to make a suite pass.**
- **Scope discipline.** The pre-existing `react-refresh/only-export-components` warning in `ConversationPane.tsx` is out of scope. Leave it.
- **Commands:** `yarn vitest run` (frontend tests), `make lint-ui` (tsc + eslint + knip), `make test` / `make lint` (both halves).

---

### Task 1: Make the QueryClient per-mount, with docs and comments in the same commit

**Files:**
- Modify: `src/App.tsx:15` (delete module-scope client), `src/App.tsx:190-196` (App component). Line 1 needs no change — `useState` is already imported.
- Modify/Test: `src/App.test.tsx` — add one test; correct comments at `174-176` and `200-202`
- Modify: `CLAUDE.md:133-135`

**Interfaces:**
- Consumes: nothing from earlier tasks — this is the only task.
- Produces: `App`'s default export signature is unchanged (`() => JSX.Element`, no props). No new exported symbols. Deliberately no `createQueryClient()` factory and no injectable `client` prop — the spec rejected that option because it adds a production API that exists only for tests.

---

- [ ] **Step 1: Write the failing test**

Add to `src/App.test.tsx`, inside the existing `describe("App", ...)` block. Place it immediately after the `"renders sessions returned by the backend"` test (currently ends line 70) so related cache behaviour reads together.

```tsx
  it("does not serve one mount's cached sessions to the next", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    const first = render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    first.unmount();

    // A promise that never settles holds the second mount in its loading
    // state. That is the whole mechanism of this test: while the second fetch
    // is in flight, the ONLY thing that could put "Fix the parser" on screen
    // is a query cache shared with the first mount. Let the second fetch
    // resolve instead and the assertion passes either way, because fresh data
    // overwrites stale data and the leak becomes invisible.
    listSessions.mockReturnValue(new Promise(() => {}));
    render(<App />);

    // Checked synchronously, not inside waitFor: react-query serves a cache
    // hit on the first paint, so a leak is present immediately or not at all.
    // waitFor would retry until the absence became true and hide the bug.
    expect(screen.queryByText("Fix the parser")).toBeNull();
  });
```

Three things a reviewer should be able to check, and why each is load-bearing:

1. **`first.unmount()`** — Testing Library's automatic cleanup runs between `it` blocks, not within one. Both mounts live in a single test, so the first must be torn down explicitly. Without it, the first mount is still on screen and the assertion fails for a reason that has nothing to do with caching.
2. **`new Promise(() => {})`** — never resolves, never rejects. `retry: false` is already set on the client, so nothing retries behind it.
3. **`queryByText`, not `getByText`** — `getByText` throws when it finds nothing, which is the passing case here. `queryByText` returns `null`, which is what `toBeNull()` needs.

- [ ] **Step 2: Run the test and confirm it fails for the intended reason**

```bash
yarn vitest run src/App.test.tsx -t "does not serve one mount"
```

Expected: **FAIL**, with the assertion reporting that the element was found when `null` was expected — something of the form `expected <div …>Fix the parser</div> to be null`.

**Confirm the failure message is that assertion.** If instead it fails with "Unable to find an element", a timeout, or a mock error, the test is broken rather than the code — fix the test before continuing. CLAUDE.md: *a test that fails because of a typo has told you nothing.*

- [ ] **Step 3: Implement the fix**

**No import change is needed.** Line 1 already reads:

```ts
import { useEffect, useMemo, useRef, useState } from "react";
```

`useState` is in scope. Leave line 1 alone.

Delete the module-scope client at line 15:

```ts
const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
```

Replace the `App` component at the bottom of the file:

```tsx
export default function App() {
  // Per mount, not module scope: a module-level client shares one cache across
  // every render in a test run, so a test could see the previous test's
  // sessions. `useState` with a lazy initializer -- not `useMemo`, which React
  // is permitted to discard and recompute, which would silently reset the
  // cache mid-session.
  const [client] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  );
  return (
    <QueryClientProvider client={client}>
      <Shell />
    </QueryClientProvider>
  );
}
```

The arrow inside `useState` is required. `useState(new QueryClient(...))` constructs a client on every render and throws it away.

- [ ] **Step 4: Run the test and confirm it passes**

```bash
yarn vitest run src/App.test.tsx -t "does not serve one mount"
```

Expected: **PASS** (1 passed).

- [ ] **Step 5: Prove the test discriminates**

The repo requires this rather than assuming it. Temporarily restore the module-scope client — put back the `const client = ...` line and change `App` to use it, leaving the new test untouched — then:

```bash
yarn vitest run src/App.test.tsx -t "does not serve one mount"
```

Expected: **FAIL** on the `toBeNull()` assertion, identical to Step 2.

Then restore the fix and re-run to confirm PASS. Do not skip this because Step 2 already failed: Step 2 proved the test fails without the fix, and this proves it fails *because of the singleton specifically* rather than because of anything else changed since.

- [ ] **Step 6: Correct the stale comments in `src/App.test.tsx`**

At `174-176`, replace:

```tsx
    // Wait for this test's own fixture (the "Managed" badge), not just the
    // title text, since a prior test's cached query result can otherwise
    // still be on screen when this render first paints.
```

with:

```tsx
    // Wait on this test's own fixture (the "Managed" badge) rather than the
    // shared title text. Each mount gets its own QueryClient, so this is no
    // longer guarding against another test's cache -- it is the assertion
    // that proves *this* fixture rendered. Keep it specific.
```

At `200-202`, replace:

```tsx
    // Wait for this test's own fixture title, not just any stale cached
    // session, since a prior test's cached query result can otherwise still
    // be on screen when this render first paints.
```

with:

```tsx
    // Wait on this test's own fixture title rather than shared text. Each
    // mount gets its own QueryClient, so this is not guarding against a prior
    // test's cache -- it is the assertion that proves this fixture rendered.
    // Keep it specific.
```

**Do not change the `waitFor` assertions themselves.** Only the comments change. Waiting on fixture-specific text is correct practice independent of cache lifetime; relaxing these to shared text would walk into the repo's documented failure #2 — *assert on something only the component under test can render*.

- [ ] **Step 7: Update `CLAUDE.md:133-135`**

Replace:

```markdown
`App.tsx`'s `QueryClient` is a module-level singleton that is not cleared between tests, so a
render can briefly show a previous test's cached data. Wait on fixture-specific text rather than
text shared across fixtures. Tracked in #7.
```

with:

```markdown
`App.tsx` builds its `QueryClient` inside the component, so every `render(<App />)` starts with an
empty cache and no test can see another's data. Build it with `useState`, never `useMemo` — React
is permitted to discard a memoized value and recompute it, which would reset the cache mid-session.
Still prefer waiting on fixture-specific text over text shared across fixtures: an assertion only
the fixture under test can satisfy is what makes a test discriminate.
```

- [ ] **Step 8: Verify no stale reference survives anywhere**

Mechanical check, not from memory — the earlier scoping pass missed the two comments by reasoning from the issue text instead of searching:

```bash
grep -rn -i 'module-level singleton\|module scope.*QueryClient\|prior test.*cach\|previous test.*cach\|Tracked in #7' \
  src CLAUDE.md CONTRIBUTING.md README.md
```

Expected: **no matches.** Any hit is a doc or comment still describing the old behaviour and must be corrected before committing.

- [ ] **Step 9: Run the full frontend suite and lint**

```bash
yarn vitest run
make lint-ui
```

Expected: **147 tests passed** (146 baseline + 1 new), 17 files. `make lint-ui` exits 0 with exactly one warning — the pre-existing `react-refresh/only-export-components` in `ConversationPane.tsx`. Any new warning is yours; fix it.

- [ ] **Step 10: Run the Rust half**

Untouched by this change, but `make test` and `make lint` must both pass before anything is considered done.

```bash
make test-rust
make lint-rust
```

Expected: PASS, 0 failures.

- [ ] **Step 11: Commit**

Code, test, and docs in one commit — the repo requires documentation to land with the change it describes.

```bash
git add src/App.tsx src/App.test.tsx CLAUDE.md
git commit -m "fix: build the QueryClient per mount, not at module scope

A module-level client shares one cache across every render in a test run,
so a render could briefly show a previous test's sessions before its own
fixture resolved. Moving construction into the component ties the cache's
lifetime to the mount. Production is unaffected: main.tsx mounts App once.

useState, not useMemo -- React may discard a memoized value and recompute
it, which would silently reset the query cache mid-session.

The new test holds the second mount in its loading state with a promise
that never settles. That window is the only point the leak is observable;
let the second fetch resolve and fresh data overwrites stale data, and the
test passes with or without the fix.

Closes #7"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Covered by |
|---|---|
| The change (`useState` lazy initializer) | Step 3 |
| Lazy initializer is load-bearing | Step 3, inline note |
| `useState` not `useMemo`, with reason | Step 3 comment, Step 7 `CLAUDE.md`, commit body |
| Production impact: none | Step 3 comment, commit body |
| The test, incl. never-resolving promise | Step 1 |
| Verification the test fails for the right reason | Steps 2 and 5 |
| `CLAUDE.md:133-135` rewrite | Step 7 |
| In-code comments at 174-176 and 200-202 | Step 6 |
| Assertions unchanged | Step 6, explicit instruction |
| `ConversationPane.test.tsx:27` untouched | Global Constraints; no step touches it |
| Success criterion: mechanical grep | Step 8 |
| Success criterion: `make test` + `make lint` | Steps 9 and 10 |

No gaps.

**Placeholder scan:** No TBD/TODO. Every code step carries the literal code. Every command carries its expected output.

**Type consistency:** `App` keeps the signature `() => JSX.Element` with no props throughout. No new exported symbols are introduced in any step, so no cross-task naming can drift. `client` is local to `App` and referenced only by the `QueryClientProvider` in the same block.
