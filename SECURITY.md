# Security Policy

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately through GitHub's
[security advisory form](https://github.com/pktstorm/claudron/security/advisories/new). We will
acknowledge the report and let you know whether we consider it a vulnerability and what we plan
to do about it.

## Scope

Claudron runs locally and reads your own Claude Code transcripts. It has no server component and
sends nothing over the network except through `gh`, which you authenticate yourself.

Findings we are particularly interested in:

- **Command injection** through a session's working directory, branch name, or any other value
  read from a transcript. These are attacker-influenceable if a transcript is, and they are
  interpolated into AppleScript and passed to subprocesses.
- **Unintended file or directory deletion.** Worktree removal is the only destructive action;
  it uses `git worktree remove` and refuses when the target is dirty, is not a worktree, or has
  a live process inside it.
- **Reading or writing outside the expected directories** — `~/.claude/projects` and Claudron's
  own annotation store.

Out of scope: anything requiring an attacker to already have code execution as your user, since
at that point they can read the transcripts directly.
