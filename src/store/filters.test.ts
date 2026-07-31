import { describe, it, expect } from "vitest";
import { applyFilters, type Filters } from "./filters";
import type { Session } from "../types";

const base: Session = {
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
};

const empty: Filters = { search: "", liveness: null, status: null };

describe("applyFilters", () => {
  it("returns everything when no filters are set", () => {
    expect(applyFilters([base], empty)).toHaveLength(1);
  });

  it("matches search against the title", () => {
    expect(applyFilters([base], { ...empty, search: "parser" })).toHaveLength(1);
    expect(applyFilters([base], { ...empty, search: "nomatch" })).toHaveLength(0);
  });

  it("matches search against the project label", () => {
    expect(applyFilters([base], { ...empty, search: "repo" })).toHaveLength(1);
  });

  it("matches search against notes", () => {
    const withNotes = { ...base, annotation: { ...base.annotation, notes: "check migration" } };
    expect(applyFilters([withNotes], { ...empty, search: "migration" })).toHaveLength(1);
  });

  it("search is case insensitive", () => {
    expect(applyFilters([base], { ...empty, search: "PARSER" })).toHaveLength(1);
  });

  it("filters by liveness", () => {
    expect(applyFilters([base], { ...empty, liveness: "legacy" })).toHaveLength(0);
    expect(applyFilters([base], { ...empty, liveness: "idle" })).toHaveLength(1);
  });

  it("filters by manual status", () => {
    const blocked = { ...base, annotation: { ...base.annotation, status: "blocked" as const } };
    expect(applyFilters([blocked], { ...empty, status: "blocked" })).toHaveLength(1);
    expect(applyFilters([base], { ...empty, status: "blocked" })).toHaveLength(0);
  });

  it("combines filters with AND", () => {
    const blocked = { ...base, annotation: { ...base.annotation, status: "blocked" as const } };
    expect(applyFilters([blocked], { search: "parser", liveness: "idle", status: "blocked" })).toHaveLength(1);
    expect(applyFilters([blocked], { search: "parser", liveness: "legacy", status: "blocked" })).toHaveLength(0);
  });
});
