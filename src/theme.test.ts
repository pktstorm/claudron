import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import tauriConf from "../src-tauri/tauri.conf.json?raw";
import indexHtml from "../index.html?raw";

// `.css` cannot be read with `?raw`: the Tailwind Vite plugin intercepts CSS
// requests and hands back an empty string, so the assertion would compare "" to
// "" and pass while proving nothing.
const readCss = (p: string) => readFileSync(new URL(p, import.meta.url), "utf8");
const indexCss = readCss("./index.css");
const tailwindTheme = readCss("../node_modules/tailwindcss/theme.css");

/**
 * The app background, the window chrome and the splash are the same colour, and
 * nothing at runtime notices when they stop being. The seam shows only as a
 * flash on the first frame at launch -- precisely when nobody is reading a test
 * report.
 *
 * Each assertion compares two INDEPENDENT origins. Deriving both sides from one
 * file would let them drift together and still pass, which is how a broken path
 * guard once survived mutation testing in this repo.
 *
 * Files are pulled in with Vite's `?raw` rather than `node:fs`: this project
 * deliberately has no `@types/node` (see the `@ts-expect-error` on `process` in
 * vite.config.ts), and `?raw` needs no new dependency.
 */
function cssVar(source: string, name: string): string {
  const m = source.match(new RegExp(`${name}:\\s*([^;]+);`));
  if (!m) throw new Error(`${name} not found`);
  return m[1].trim();
}

describe("theme tokens", () => {
  it("resolves --background to Tailwind's own neutral-900", () => {
    // Origin 1: the token this project declares.
    // Origin 2: Tailwind's palette definition. Hardcoding the oklch literal on
    // both sides would still pass if Tailwind changed the palette underneath.
    const declared = cssVar(indexCss, "--background");
    const tailwind = cssVar(tailwindTheme, "--color-neutral-900");

    expect(declared).toBe(tailwind);
  });

  it("keeps the window chrome and the splash on the same colour", () => {
    // Origin 1: the Tauri window's backgroundColor, painted before any HTML.
    // Origin 2: the inline splash style, inline so it paints on the first frame.
    const chrome = JSON.parse(tauriConf).app.windows[0].backgroundColor as string;
    const splash = indexHtml.match(/background:\s*(#[0-9a-fA-F]{6})/)?.[1];

    expect(splash).toBeDefined();
    expect(splash?.toLowerCase()).toBe(chrome.toLowerCase());
  });
});
