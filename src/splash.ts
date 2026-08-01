/// Dismiss the launch splash defined in `index.html`.
///
/// The splash exists because `<body>` is empty until React mounts, so the
/// webview paints its own default white — for as long as the first session
/// scan takes. A cold scan of the real tree measured **11.6 s** for 38 sessions
/// across ~1461 transcripts, which is why this cannot be a late-appearing
/// spinner.
///
/// Dismissal is driven by the app, not a timer: a fixed delay would either
/// uncover a still-empty UI on a slow machine or linger on a fast one.

const SPLASH_ID = "splash";
const HIDING = "claudron-hiding";
const PROGRESS_ID = "splash-progress-bar";

/// How long the fade-out lasts. Must match the CSS transition in `index.html`.
const FADE_MS = 400;

/// Update the splash's progress bar.
///
/// Writes to the DOM directly rather than through React, because the splash
/// lives in `index.html` and must work before — and independently of — the
/// React tree. No-ops once the splash has been dismissed.
///
/// `fraction` is clamped: a live session can grow its transcript mid-scan, so
/// bytes-done can exceed the total measured at enumeration time. A bar that
/// overshoots its track looks broken.
export function setSplashProgress(fraction: number, doc: Document = document): void {
  const el = doc.getElementById(SPLASH_ID);
  if (!el || el.classList.contains(HIDING)) return;

  const bar = doc.getElementById(PROGRESS_ID);
  if (!bar) return;

  const pct = Math.min(100, Math.max(0, fraction * 100));
  bar.style.width = `${pct}%`;
  // The bar is hidden until there is something to report, so a warm start
  // (~19ms) never flashes an empty track.
  bar.parentElement?.classList.add("claudron-visible");
}

/// Hide the splash and remove it from the DOM.
///
/// Removing it matters: a `position: fixed; inset: 0` element left in place
/// would swallow every click even at zero opacity. Safe to call more than once
/// and safe when no splash exists (tests, hot reload).
export function dismissSplash(doc: Document = document, fadeMs: number = FADE_MS): void {
  const el = doc.getElementById(SPLASH_ID);
  if (!el || el.classList.contains(HIDING)) return;

  el.classList.add(HIDING);

  const remove = () => el.remove();
  // `transitionend` is the natural signal, but it never fires when the element
  // is not rendered — a background window, or `prefers-reduced-motion` turning
  // the transition off. The timeout guarantees removal either way.
  el.addEventListener("transitionend", remove, { once: true });
  setTimeout(remove, fadeMs + 100);
}
