import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { HookSettings } from "./HookSettings";
import * as api from "../api/hooks";
import type { HookPlan } from "../types/hooks";

const plan = (over: Partial<HookPlan> = {}): HookPlan => ({
  state: "notInstalled",
  settingsPath: "/Users/s/.claude/settings.json",
  scriptPath: "/Users/s/.claude/claudron/session-hook.sh",
  events: ["SessionStart", "SessionEnd"],
  settingsSnippet: '{\n  "hooks": {\n    "SessionStart": []\n  }\n}',
  ...over,
});

beforeEach(() => vi.restoreAllMocks());

describe("HookSettings", () => {
  it("does NOT install anything just by rendering", async () => {
    // The whole point of a consent flow: settings.json belongs to Claude Code,
    // and nothing may be written to it without an explicit click.
    const install = vi.spyOn(api, "installHooks");
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    render(<HookSettings />);
    await screen.findByText(/Not installed/);
    expect(install).not.toHaveBeenCalled();
  });

  it("installs only when the button is clicked", async () => {
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    const install = vi.spyOn(api, "installHooks").mockResolvedValue(plan({ state: "installed" }));
    render(<HookSettings />);
    fireEvent.click(await screen.findByRole("button", { name: /Install hook/ }));
    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/^Installed$/)).toBeDefined();
  });

  it("names the file it will modify before doing anything", async () => {
    // The user should never have to guess which file is being changed.
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    render(<HookSettings />);
    expect(await screen.findByText(/\/Users\/s\/\.claude\/settings\.json/)).toBeDefined();
  });

  it("can show the exact JSON that will be added", async () => {
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    render(<HookSettings />);
    const toggle = await screen.findByRole("button", { name: /Show exactly what/ });
    expect(screen.queryByText(/"hooks"/)).toBeNull();
    fireEvent.click(toggle);
    expect(await screen.findByText(/"hooks"/)).toBeDefined();
  });

  it("offers removal once installed, not installation", async () => {
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan({ state: "installed" }));
    render(<HookSettings />);
    expect(await screen.findByRole("button", { name: /Remove hook/ })).toBeDefined();
    expect(screen.queryByRole("button", { name: /Install hook/ })).toBeNull();
  });

  it("offers a repair when only some events are registered", async () => {
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan({ state: "partial" }));
    render(<HookSettings />);
    expect(await screen.findByRole("button", { name: /Repair hook/ })).toBeDefined();
  });

  it("surfaces a failure instead of silently doing nothing", async () => {
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    vi.spyOn(api, "installHooks").mockRejectedValue(
      new Error("settings.json is not valid JSON. Refusing to overwrite it."),
    );
    render(<HookSettings />);
    fireEvent.click(await screen.findByRole("button", { name: /Install hook/ }));
    expect(await screen.findByRole("alert")).toBeDefined();
    expect(screen.getByRole("alert").textContent).toContain("Refusing to overwrite");
  });

  it("explains that Claudron still works without the hook", async () => {
    // Declining must not read as breaking the app.
    vi.spyOn(api, "hookStatus").mockResolvedValue(plan());
    render(<HookSettings />);
    expect(await screen.findByText(/without the hook it falls back/i)).toBeDefined();
  });
});
