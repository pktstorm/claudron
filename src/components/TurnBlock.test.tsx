import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TurnBlock } from "./TurnBlock";
import type { Turn, ToolCall } from "../types/conversation";

const call = (over: Partial<ToolCall> = {}): ToolCall => ({
  id: "t1",
  name: "Bash",
  description: "List files",
  result: "total 0",
  isError: false,
  agentId: null,
  ...over,
});

const turn = (over: Partial<Turn> = {}): Turn => ({
  uuid: "u1",
  role: "assistant",
  timestamp: "2026-07-31T00:00:00Z",
  blocks: [{ kind: "text", text: "Hello there" }],
  model: "claude-opus-5",
  usage: null,
  ...over,
});

describe("TurnBlock", () => {
  it("renders prose text", () => {
    render(<TurnBlock turn={turn()} onExpandSubagent={vi.fn()} />);
    expect(screen.getByText("Hello there")).toBeDefined();
  });

  it("shows a tool call collapsed, using its description", () => {
    render(
      <TurnBlock turn={turn({ blocks: [{ kind: "tool", call: call() }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(screen.getByText("List files")).toBeDefined();
    // Output is hidden until expanded.
    expect(screen.queryByText("total 0")).toBeNull();
  });

  it("falls back to the tool name when there is no description", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ description: null }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    expect(screen.getByText("Bash")).toBeDefined();
  });

  it("reveals the output when the tool call is clicked", () => {
    render(
      <TurnBlock turn={turn({ blocks: [{ kind: "tool", call: call() }] })} onExpandSubagent={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText("total 0")).toBeDefined();
  });

  it("marks an errored tool call", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ isError: true }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    expect(screen.getByText(/failed/i)).toBeDefined();
  });

  it("says so when a tool call produced no result", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ result: null }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText(/no result/i)).toBeDefined();
  });

  it("offers to expand a subagent when one is linked", () => {
    const onExpand = vi.fn();
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ agentId: "a1" }) }] })}
        onExpandSubagent={onExpand}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));
    expect(onExpand).toHaveBeenCalledWith("a1");
  });

  it("shows the model on an assistant turn", () => {
    render(<TurnBlock turn={turn()} onExpandSubagent={vi.fn()} />);
    expect(screen.getByText("claude-opus-5")).toBeDefined();
  });

  it("keeps a tool call expanded when its result arrives later", () => {
    // The measured common case: 60% of tool calls outlast the poll interval,
    // so the result patches into a turn the user may already have expanded.
    const before = turn({ blocks: [{ kind: "tool", call: call({ result: null }) }] });
    const { rerender } = render(<TurnBlock turn={before} onExpandSubagent={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText(/no result/i)).toBeDefined();

    // Same call id, now with a result — as applyDelta would produce.
    const after = turn({ blocks: [{ kind: "tool", call: call({ result: "all green" }) }] });
    rerender(<TurnBlock turn={after} onExpandSubagent={vi.fn()} />);

    expect(screen.getByText("all green")).toBeDefined();
    expect(screen.queryByText(/no result/i)).toBeNull();
  });
});

describe("markdown rendering", () => {
  // Claude Code emits markdown. Rendered as plain text, `**bold**` and fenced
  // code blocks reach the user as literal characters -- these tests assert on
  // the produced ELEMENTS, because asserting on text alone passes either way.

  it("renders bold as a <strong> element, not literal asterisks", () => {
    const { container } = render(
      <TurnBlock turn={turn({ blocks: [{ kind: "text", text: "a **bold** word" }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.textContent).not.toContain("**");
  });

  it("renders a fenced code block as <pre><code>, preserving its content", () => {
    const md = "run this:\n\n```sh\nyarn install\n```";
    const { container } = render(
      <TurnBlock turn={turn({ blocks: [{ kind: "text", text: md }] })} onExpandSubagent={vi.fn()} />,
    );
    const code = container.querySelector("pre code");
    expect(code).not.toBeNull();
    expect(code?.textContent).toContain("yarn install");
    expect(container.textContent).not.toContain("```");
  });

  it("renders list items as <li>, not as leading dashes", () => {
    const { container } = render(
      <TurnBlock turn={turn({ blocks: [{ kind: "text", text: "- one\n- two" }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(container.querySelectorAll("li")).toHaveLength(2);
  });

  it("renders a GFM table, which plain markdown would not", () => {
    const md = "| a | b |\n|---|---|\n| 1 | 2 |";
    const { container } = render(
      <TurnBlock turn={turn({ blocks: [{ kind: "text", text: md }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(container.querySelector("table")).not.toBeNull();
    expect(container.querySelectorAll("tbody tr")).toHaveLength(1);
  });

  it("does not execute raw HTML embedded in transcript text", () => {
    // Transcript content is untrusted: it carries whatever the model wrote and
    // whatever tool output was captured. Raw HTML must never reach the DOM.
    const md = '<img src=x onerror="window.__pwned=1"> plain';
    const { container } = render(
      <TurnBlock turn={turn({ blocks: [{ kind: "text", text: md }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("plain");
  });

  it("still renders tool calls alongside markdown text", () => {
    // Guards the existing behaviour: markdown must not displace tool blocks.
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "text", text: "**go**" }, { kind: "tool", call: call() }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    // ToolCallBlock renders `description ?? name`, so this fixture shows its description.
    expect(screen.getByText("List files")).toBeDefined();
  });
});
