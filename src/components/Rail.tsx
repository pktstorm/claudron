export type ViewId = "sessions" | "dashboard";

const VIEWS: { id: ViewId; label: string; glyph: string }[] = [
  { id: "sessions", label: "Sessions", glyph: "☰" },
  { id: "dashboard", label: "Dashboard", glyph: "▦" },
];

/**
 * Top-level view switcher.
 *
 * A rail rather than a header tab bar: the conversation pane is the tightest
 * thing on screen, and a horizontal strip would cost it vertical space on every
 * view forever.
 *
 * The active item carries `aria-current="page"`. Marking it by colour alone
 * would leave assistive technology unable to report which view is showing, and
 * would leave tests asserting on class strings rather than behaviour.
 */
export function Rail({
  active,
  onSelect,
}: {
  active: ViewId;
  onSelect: (v: ViewId) => void;
}) {
  return (
    <nav
      aria-label="Views"
      className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-neutral-800 py-2"
    >
      {VIEWS.map((v) => (
        <button
          key={v.id}
          type="button"
          title={v.label}
          aria-label={v.label}
          aria-current={active === v.id ? "page" : undefined}
          onClick={() => onSelect(v.id)}
          className={`flex h-9 w-9 items-center justify-center rounded text-base ${
            active === v.id
              ? "bg-neutral-800 text-neutral-100"
              : "text-neutral-500 hover:text-neutral-300"
          }`}
        >
          <span aria-hidden="true">{v.glyph}</span>
        </button>
      ))}
    </nav>
  );
}
