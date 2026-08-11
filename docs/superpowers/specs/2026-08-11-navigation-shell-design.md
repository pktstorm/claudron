# Claudron — Top-Level Navigation Shell

**Date:** 2026-08-11
**Status:** Draft, pending review
**Scope:** Small. One component split, two new components, no new dependencies.
**Issue:** [#75](https://github.com/pktstorm/claudron/issues/75)
**Blocks:** [#15](https://github.com/pktstorm/claudron/issues/15) (stats dashboard)

## Summary

`App.tsx` renders one fixed layout with no view switching of any kind. A dashboard is not a new tab
inside an existing shell — the shell does not exist. This adds it: a slim icon rail, two views, and
a clean split of what is global from what belongs to the session view.

It ships **no dashboard content**. That is #15.

## What `Shell` currently owns

All 202 lines of `App.tsx` are one component plus a thin `App` wrapper. `Shell` holds seven
concerns:

| | Concern | Belongs to |
|---|---|---|
| 1 | sessions query + 10 s poll | session view |
| 2 | filters | session view |
| 3 | selection, and the during-render reconciliation that drops a stale draft | session view |
| 4 | annotation draft, debounced save, `saveError` | session view |
| 5 | `showDetails` / `showSettings` | session view |
| 6 | **splash dismissal and progress** | **global** |
| 7 | the two-pane layout | session view |

Six move to `SessionsView` verbatim. The seventh is why this change needs a design.

## The trap: the splash is dismissed by the sessions query

```ts
const settled = !isLoading || !!error;
useEffect(() => { if (settled) dismissSplash(); }, [settled]);
```

That coupling is deliberate and well-reasoned. A cold scan measured **11.6 s** against **~19 ms**
warm, so no fixed delay is right on both machines, and dismissing on `error` too stops a failed scan
leaving the splash covering the error underneath.

But it means the splash is dismissed by *the session view*. The moment that view stops being the
thing that always mounts, launching into any other view leaves the splash up forever — and per
`splash.ts`'s own comment, a `position: fixed; inset: 0` element left in place "would swallow every
click even at zero opacity". The app would look frozen at launch, with no error anywhere.

No existing test would catch it: nothing today exercises a non-session view.

**Resolution.** `App` owns the splash and runs the sessions query itself, purely to know when the
first scan settles. `SessionsView` runs the same query for its data.

**This costs nothing, verified rather than assumed.** Two components sharing the `["sessions"]` key
under one `QueryClientProvider` produce exactly **one** fetch — confirmed with a throwaway test
counting `queryFn` invocations before this design was written.

## Architecture

```
App.tsx
├── QueryClientProvider                 unchanged, still per-mount (see its comment)
├── useState<"sessions" | "dashboard">
├── useQuery(["sessions"])              only to know when the first scan settles
├── splash dismissal + progress effects
└── <Rail /> + the active view
     ├── SessionsView    today's Shell, minus the splash
     └── DashboardView   placeholder
```

### No router

Two views and a `useState`. #75 warns against reaching for a router reflexively, and a dependency
that owns navigation is hard to remove. Revisit if a third view or deep-linking arrives.

### Rail

`src/components/Rail.tsx`: a `<nav>` of buttons, `aria-current="page"` on the active one — a real
accessibility affordance and the test hook.

Plain buttons, **not** the `Toggle` added in #76. This branches off `main`, where shadcn does not
exist yet. If both land, the Rail can adopt it later; coupling the two would make each wait for the
other.

### DashboardView

A real component rendering "Nothing here yet". It gives #15 a mount point, proves switching works,
and depends on nothing — wiring it to `dashboard_stats()` would block this behind the unmerged #71.

## Testing

`splash.ts` takes `doc: Document = document` and keys off `#splash`, so the splash can be tested by
injecting the element — no mocking.

1. **The splash dismisses when the app opens on the dashboard.** The discriminating test: it fails
   if the splash logic stays in `SessionsView`, which is the exact bug this design exists to
   prevent.
2. **Switching to the dashboard unmounts the session list.** Asserted on text only `SessionsView`
   can render. Per `CLAUDE.md`, an App-level test once asserted `getByText("main")` and passed
   against the always-mounted sidebar while the component under test was replaced by a stub — the
   assertion here must not be satisfiable by the rail or the dashboard.
3. **The Rail marks the active view** with `aria-current`.
4. **The existing `App.test.tsx` suite passes unchanged** — the regression check on all six moved
   concerns.

Per `CLAUDE.md`, `@testing-library/jest-dom` matchers are not registered: use `toBeDefined()`,
`toBeNull()`, `toBe()`, `toEqual()`.

## Out of scope

- Dashboard content — #15.
- Keyboard shortcuts and a command palette — #24.
- Relocating Settings out of the sidebar footer — #22.
- Remembering the selected view across launches. It needs somewhere to persist, which is #42.

## Success criteria

- `make test` and `make lint` both pass.
- The app launches on the sessions view looking exactly as it does today.
- Switching to the dashboard replaces the whole layout — no session list, no 80-column sidebar.
- Opening on the dashboard still dismisses the splash, proven by a test that fails if the splash
  logic stays with the session view.
