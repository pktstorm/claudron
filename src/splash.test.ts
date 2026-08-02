import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { dismissSplash, setSplashProgress } from "./splash";

function mountSplash(): HTMLElement {
  document.body.innerHTML =
    `<div id="splash"><img alt="Claudron" />` +
    `<div id="splash-progress"><div id="splash-progress-bar"></div></div>` +
    `</div><div id="root"></div>`;
  return document.getElementById("splash") as HTMLElement;
}

const bar = () => document.getElementById("splash-progress-bar") as HTMLElement;
const track = () => document.getElementById("splash-progress") as HTMLElement;

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

describe("setSplashProgress", () => {
  it("sets the bar width from the fraction", () => {
    mountSplash();
    setSplashProgress(0.42);
    expect(bar().style.width).toBe("42%");
  });

  it("reveals the track only once there is progress to show", () => {
    // A warm scan is ~19ms; flashing an empty track for it looks like a glitch.
    mountSplash();
    expect(track().classList.contains("claudron-visible")).toBe(false);
    setSplashProgress(0.1);
    expect(track().classList.contains("claudron-visible")).toBe(true);
  });

  it("clamps above 100%, because a live session can grow mid-scan", () => {
    // bytesTotal is measured at enumeration; a session writing during the scan
    // pushes bytesDone past it. A bar overflowing its track looks broken.
    mountSplash();
    setSplashProgress(1.8);
    expect(bar().style.width).toBe("100%");
  });

  it("clamps below zero", () => {
    mountSplash();
    setSplashProgress(-0.5);
    expect(bar().style.width).toBe("0%");
  });

  it("does nothing once the splash is dismissed", () => {
    // A scan in flight keeps emitting after the app has taken over; a late
    // event must not touch a splash that is on its way out.
    mountSplash();
    dismissSplash();
    setSplashProgress(0.9);
    expect(bar().style.width).not.toBe("90%");
  });

  it("does not throw when there is no splash", () => {
    document.body.innerHTML = `<div id="root"></div>`;
    expect(() => setSplashProgress(0.5)).not.toThrow();
  });
});
