# Contributing to Claudron

Thanks for your interest. Issues and pull requests are welcome.

Claudron is maintained by a small team, and `main` is protected — every change lands through a
pull request. External contributors should fork the repository and open a PR from their fork.

## Getting set up

You will need macOS, [Node.js](https://nodejs.org/) 18+, [Yarn](https://yarnpkg.com/), and a
stable [Rust](https://rustup.rs/) toolchain.

```bash
git clone https://github.com/pktstorm/claudron.git
cd claudron
yarn install
make dev
```

## Before you open a pull request

```bash
make test   # Rust and frontend suites
make lint   # clippy -D warnings, then tsc --noEmit
```

Both must pass. `make lint` runs clippy with `-D warnings`, so warnings fail the build.

Some tests are `#[ignore]`d because they need a real git repository with a real GitHub remote.
Point them at one rather than hardcoding a path:

```bash
CLAUDRON_TEST_REPO=~/code/some-repo \
  cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
```

## How this codebase is built

A few conventions are load-bearing. They exist because breaking them has caused real bugs.

**All `git`, `gh`, and process work lives in Rust.** The React layer reaches the backend only
through `src/api/`. It never spawns a process and never touches the filesystem.

**Every subprocess goes through the bounded runner in `src-tauri/src/git/run.rs`.** Never call
`std::process::Command` directly. The runner enforces a timeout and drains both pipes on
separate threads — a child that writes past the pipe buffer while nothing reads it blocks
forever.

**Nullable values are `T | null`, never `T?`.** The Rust types serialize `Option<T>` as an
explicit `null`, and the distinction matters: `ahead: null` means "no upstream configured" while
`ahead: 0` means "in sync". Rendering them the same way tells the user something false.

**Use `??` for fallbacks, never `||`.** `0` and `""` are legitimate values that `||` discards.

**Never mutate process-global state in a test.** `cargo test` runs this crate's tests
concurrently in one process. A test that set `PATH` once raced unrelated tests and failed about
one run in seven. If you need to test error mapping, extract a pure function and test that.

**A single green run does not prove a race is absent.** If a change touches shared state,
threads, or subprocess spawning:

```bash
for i in $(seq 1 20); do
  cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=8 \
    || echo "FAILED ON $i"
done
```

**Canonicalize both sides before comparing paths.** `lsof` reports fully-resolved paths
(`/private/tmp/...`) while recorded session directories are not resolved (`/tmp/...`). On macOS
those name the same directory and compare unequal.

## Tests

Write the failing test first, and confirm it fails for the reason you expect before implementing.

Then check that it would actually catch a regression. Break the implementation deliberately and
confirm the test fails — a test that passes regardless of what the code does is worse than no
test, because it reads as coverage. Several tests in this project's history passed while the
thing they claimed to cover was broken:

- one compared a value against itself, so it could never detect a divergence between two sources
- one asserted on text that a different, always-mounted component happened to render
- one asserted only that a call returned `Ok`, which was true whether or not it found anything

If you find that a test does not genuinely discriminate, say so in the pull request rather than
counting it as coverage.

**Never hardcode a live external identifier.** A pull request number or branch name pinned today
merges tomorrow, and the test then passes while proving nothing. Discover a valid value at
runtime and make "none available" a loud skip.

## Pull requests

- Keep them focused. One concern per PR.
- Explain what breaks without the change, not only what the change does.
- Say what you verified and how. If something is untested or uncertain, say that too.
- Do not weaken or delete an existing test to make a suite pass.

## Reporting bugs

Open an issue with the version of Claudron and macOS, what you expected, what happened, and the
steps to reproduce. If a session is involved, the relevant behaviour of that session helps —
please redact anything sensitive from transcript excerpts.

## Security

Please do not open a public issue for a security problem. See [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
