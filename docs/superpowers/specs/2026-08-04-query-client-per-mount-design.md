# Claudron — QueryClient Per Mount

**Date:** 2026-08-04
**Status:** Approved, pending implementation plan
**Scope:** Small. One component, one new test, two documentation edits. No behaviour change in production.
**Issue:** [#7](https://github.com/pktstorm/claudron/issues/7)

## Summary

`src/App.tsx:15` constructs its `QueryClient` at module scope, so the cache outlives every
individual test and is shared across all of them. A `render(<App />)` can briefly display the
previous test's session data before its own fixture resolves.

The fix moves construction inside the component, so the cache's lifetime becomes the component's
lifetime. Production is unaffected — `main.tsx` mounts `App` exactly once — while each test render
starts with an empty cache.

## Why this is worth fixing now

It has not yet caused a failure. It was found while wiring the Git tab into the slide-over and
worked around there by waiting on fixture-specific text rather than text shared across fixtures.
That workaround is currently documented in `CLAUDE.md` as a standing rule.

The workaround only holds while every fixture stays visually distinguishable. The next App-level
test written with two similar fixtures gets a real flake, and it will present as "this assertion
sometimes sees the wrong session" — one of the harder failures to trace back to its cause, because
the offending state was created by a *different test file*.

## The change

```ts
// before — src/App.tsx:15, module scope
const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

export default function App() {
  return (
    <QueryClientProvider client={client}>
      <Shell />
    </QueryClientProvider>
  );
}
```

```ts
// after — construction owned by the component
export default function App() {
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

Two details in that diff are load-bearing.

**The lazy initializer.** `useState(new QueryClient(...))` — without the arrow — evaluates on every
render and discards the result, allocating a fresh client each time and throwing it away. Only the
function form defers construction to first render.

**`useState`, not `useMemo`.** The instinct is `useMemo(() => new QueryClient(), [])`, and it reads
as equivalent. It is not. React treats `useMemo` as a caching hint and is explicitly permitted to
discard memoized values and recompute them. A discarded client would silently reset the query cache
mid-session. `useState` guarantees the value's identity for the life of the component; `useMemo`
does not promise that.

## Production impact: none

`main.tsx` renders `<App />` once at the root. Moving construction inside it changes when the
client is built, not how many exist.

Under `React.StrictMode` the initializer is deliberately invoked twice in development and one result
is discarded. A `QueryClient` opens no subscriptions and starts no timers until a `QueryClientProvider`
mounts it, so the discarded instance is inert.

## The test

The bar is `CLAUDE.md`'s: *would this test fail if the code were wrong?* Asserting that two renders
each show their own fixture would pass with or without the fix, because the second fetch resolves
and overwrites the stale data either way. The leak is only observable in the window **before** the
second fetch settles.

So the test holds that window open:

1. Render `App` with `listSessions` resolving to a session titled `"Fix the parser"`. Wait for it.
2. Unmount.
3. Re-render `App` with `listSessions` returning a promise that never resolves.
4. Assert `"Fix the parser"` is absent.

With a module-scoped client, step 4 fails: react-query serves the cached `["sessions"]` entry
synchronously while the new fetch is in flight. With a per-mount client the second render has an
empty cache and shows its loading state.

The never-resolving promise is the mechanism that makes this discriminate, and it is why the test
cannot be simplified into "render twice and check".

**Verification:** revert the fix, confirm the new test fails *and that it fails on the missing-text
assertion* rather than on a timeout or a mock error, then restore. A test that fails for the wrong
reason has proved nothing.

## Documentation

The edit lands in the same commit as the code.

**`CLAUDE.md:133-135`** is the only place this guidance appears — `CONTRIBUTING.md` was checked and
does not mention it. The paragraph currently reads:

> `App.tsx`'s `QueryClient` is a module-level singleton that is not cleared between tests, so a
> render can briefly show a previous test's cached data. Wait on fixture-specific text rather than
> text shared across fixtures. Tracked in #7.

The constraint is gone once this lands. Leaving the warning would route the next contributor around
an obstacle that no longer exists, which this repo treats as a defect.

Waiting on specific rather than shared text remains good practice; it simply stops being a
requirement forced by shared cache state. The rewrite should say that rather than deleting the
advice outright.

## Out of scope

Existing tests that were written around the singleton are left as they are. They are correct, they
pass, and rewriting them would widen the diff into files this issue did not ask about. The
pre-existing `react-refresh/only-export-components` warning in `ConversationPane.tsx` is likewise
untouched.

## Success criteria

- `make test` passes — 146 existing tests plus the new one.
- `make lint` passes, with no new warnings beyond the pre-existing `ConversationPane.tsx` one.
- The new test has been observed failing against the unfixed code, for the intended reason.
- No `CLAUDE.md` or `CONTRIBUTING.md` guidance still describes the singleton as current.
