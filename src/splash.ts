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

/// How long the fade-out lasts. Must match the CSS transition in `index.html`.
const FADE_MS = 400;

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
