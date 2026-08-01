import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { dismissSplash } from "./splash";

function mountSplash(): HTMLElement {
  document.body.innerHTML = `<div id="splash"><img alt="Claudron" /></div><div id="root"></div>`;
  return document.getElementById("splash") as HTMLElement;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

describe("dismissSplash", () => {
  it("removes the splash from the DOM, not merely hides it", () => {
    // A `position: fixed; inset: 0` element left in place swallows every click
    // even at zero opacity, so hiding alone would leave the app unusable.
    mountSplash();
    dismissSplash();
    vi.runAllTimers();
    expect(document.getElementById("splash")).toBeNull();
  });

  it("fades before removing rather than cutting instantly", () => {
    const el = mountSplash();
    dismissSplash();
    expect(el.classList.contains("claudron-hiding")).toBe(true);
    // Still present mid-fade.
    expect(document.getElementById("splash")).not.toBeNull();
    vi.runAllTimers();
    expect(document.getElementById("splash")).toBeNull();
  });

  it("removes the splash even when the transition never fires", () => {
    // transitionend does not fire for an element that is not rendered -- a
    // background window, or prefers-reduced-motion disabling the transition.
    // Without the timeout fallback the splash would stay up forever.
    mountSplash();
    dismissSplash();
    vi.runAllTimers(); // no transitionend dispatched
    expect(document.getElementById("splash")).toBeNull();
  });

  it("removes it as soon as the transition ends, without waiting out the timer", () => {
    const el = mountSplash();
    dismissSplash();
    el.dispatchEvent(new Event("transitionend"));
    expect(document.getElementById("splash")).toBeNull();
  });

  it("is safe to call twice", () => {
    // The effect can re-run; a second call must not throw on an already-removed
    // element.
    mountSplash();
    dismissSplash();
    expect(() => dismissSplash()).not.toThrow();
    vi.runAllTimers();
    expect(document.getElementById("splash")).toBeNull();
  });

  it("is safe to call when there is no splash at all", () => {
    // Tests and hot reload render the app without index.html's markup.
    document.body.innerHTML = `<div id="root"></div>`;
    expect(() => dismissSplash()).not.toThrow();
  });

  it("leaves the rest of the document alone", () => {
    mountSplash();
    dismissSplash();
    vi.runAllTimers();
    expect(document.getElementById("root")).not.toBeNull();
  });
});
