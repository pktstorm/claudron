# Claudron — shadcn/ui Foundation

**Date:** 2026-08-11
**Status:** Draft, pending review
**Scope:** Small. Config, a token block, one vendored component, one component migrated.
**Issue:** [#13](https://github.com/pktstorm/claudron/issues/13)
**Related:** [#15](https://github.com/pktstorm/claudron/issues/15) (dashboard), [#75](https://github.com/pktstorm/claudron/issues/75) (navigation)

## Summary

Establish shadcn/ui so the dashboard (#15) and settings work have real components to build on,
without changing how the app looks and without leaving unused code behind.

## Two of #13's premises are wrong

**"A test asserts `App.tsx` still uses `bg-neutral-900`."** There is no such test. The only
`className` assertions in the suite are `SessionList.test.tsx:204,226`, and they compare two
computed version-badge classNames to each other — no literal colour appears in any test.

**The real constraint is not a test.** `#171717` is pinned in two places outside CSS:
`tauri.conf.json:22` (`backgroundColor`) and an inline `<style>` in `index.html`, which is inline
precisely so the splash renders on first paint. Tailwind v4's `neutral-900` is
`oklch(20.5% 0 none)` — the same colour. The app background, the window chrome and the splash agree
today, so any token that shifts `--background` opens a visible flash at the splash-to-app seam.
That is the constraint to respect.

## Measured starting point

| | |
|---|---|
| `src/index.css` | one line: `@import "tailwindcss";` |
| `dark:` variants in `src/` | **0** — the app is unconditionally dark |
| Colour utility classes in use | ~200, `neutral` dominant; `text-neutral-500` alone appears 30× |
| Raw `<button>` elements | 21 across 11 files |
| Existing shadcn primitives | none — no `components.json`, no `src/components/ui/` |

## The constraint that shapes the whole change: knip

`make lint` runs `knip`, and **knip fails the build on a file nothing imports.** Verified, rather
than assumed — an unused export added to `src/lib/` gives:

```
$ yarn knip   → exit 1   ("Unused files (1)")
$ make lint   → exit 2
```

So #13's plan as written — "install shadcn and its primitives" — does not build. Neither does a
foundation-only change: `cn()` with no importer fails the same way. **Every file this change adds
must have a consumer in the same change.**

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Theming | **Dark-only tokens** at `:root`; no `.dark`, no light set | The app has never had a light mode, and the chrome and splash are pinned dark outside CSS. Light tokens would style every component in a mode nobody has looked at. |
| Token values | **Derived from the existing palette**, not shadcn's defaults | Adopting shadcn's dark theme would restyle the whole app as a side effect of adding a component library. |
| What to install | Only what a real migration consumes | knip, above. |
| First consumer | `FilterBar` | 47 lines, has a test file, and its chips are a genuine shadcn use case. |
| Component | **`Toggle`**, not `Button` | The chips select and deselect a filter. `Toggle` says that, and gives `aria-pressed` for free — an accessibility improvement and the test hook the current tests lack. |

## What lands

| Added | Consumer |
|---|---|
| `components.json` | shadcn CLI config (not code; knip does not scan it) |
| `src/lib/utils.ts` — `cn()` | `ui/toggle.tsx` |
| token block in `src/index.css` | `ui/toggle.tsx` and the app shell |
| `src/components/ui/toggle.tsx` | `FilterBar.tsx` |
| deps: `clsx`, `tailwind-merge`, `class-variance-authority`, `@radix-ui/react-toggle` | the above |

Deliberately **not** added: `lucide-react` (no icons needed), and none of cards, tabs, tooltips,
popovers or switches — each would fail knip until #15 imports it. They arrive with their consumers.

## Tokens

Each token takes the value the app already uses, so the rendered result is unchanged:

```css
:root {
  --background:       oklch(20.5% 0 none);  /* neutral-900 — matches tauri.conf.json + splash */
  --foreground:       neutral-100
  --card, --popover:  neutral-900
  --secondary:        neutral-800           /* today's unselected chip background */
  --accent:           neutral-800
  --muted-foreground: neutral-500           /* the most-used text colour */
  --border, --input:  neutral-800
  --ring:             sky-500               /* today's focus ring */
  --destructive:      red-500
}
```

**Status hues stay out of the token vocabulary.** `sky` for liveness, `purple` for manual status,
`amber`/`emerald`/`red` for CI state are semantic application colours, not theme roles — shadcn has
no token for "waiting on me". They remain explicit classes, merged through `cn()`.

## The migration

`FilterBar`'s 7 chips become `Toggle`, taking `pressed={liveness === l}` and `onPressedChange`.
That expresses what the existing `onClick={() => set(x === l ? null : l)}` was simulating. The
per-group hue classes pass through `className`.

The search `<input>` stays a plain element. shadcn's `Input` would be a second vendored file with
exactly one consumer and buys nothing the current element lacks.

## Testing

The two existing `FilterBar` tests must pass **unchanged** — they are the regression check on
humanised labels and the search box's accessible name.

New tests close the gap those leave. Nothing today covers toggling at all:

- **A chip reflects its filter state as `aria-pressed`.** Fails if `pressed` is wired to the wrong
  side of the comparison — a bug that would look correct until clicked.
- **Clicking a pressed chip clears the filter** rather than re-setting it. This is the toggle-off
  round-trip through the zustand store, currently untested in either implementation.
- **`--background` still resolves to `neutral-900`.** Guards the splash seam, which is the one
  visual regression that would otherwise be noticed only by eye, and only at launch.

Per `CLAUDE.md`, `@testing-library/jest-dom` matchers are not registered: assertions use
`toBeDefined()`, `toBeNull()`, `toBe()`, `toEqual()`.

## Out of scope

- Migrating the other 20 `<button>` elements. #13's own rule: migrate existing components only when
  touching them for another reason.
- Any light theme, and any theme toggle. A real toggle would also need `tauri.conf.json` and the
  `index.html` splash changed, which is a different piece of work.
- The dashboard's components. They arrive with #15, each with its consumer.

## Success criteria

- `make test` and `make lint` both pass, knip included and unmodified.
- The app renders identically — no colour changes anywhere.
- `FilterBar`'s existing two tests pass without edits.
- Toggling a filter off is covered by a test that fails against a wrongly-wired `pressed`.

## Follow-ups to file

1. **#13 should be corrected** — the `bg-neutral-900` test it warns about does not exist, and the
   real `#171717` constraint it omits lives in `tauri.conf.json` and `index.html`.
2. The remaining 20 raw `<button>` elements are a slow migration, not a task; worth a tracking
   issue only if the inconsistency starts to bite.
