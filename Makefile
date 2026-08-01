.PHONY: dev build test test-rust test-ui lint lint-rust lint-ui fmt

dev:
	yarn tauri dev

build:
	yarn tauri build

test: test-rust test-ui

test-rust:
	cd src-tauri && cargo test

test-ui:
	yarn vitest run

lint: lint-rust lint-ui

# --all-targets lints test code too. Without it, warnings accumulate in tests
# unnoticed -- which is exactly how two of them did.
lint-rust:
	cd src-tauri && cargo fmt --check
	cd src-tauri && cargo clippy --all-targets -- -D warnings

lint-ui:
	yarn tsc --noEmit
	yarn eslint .
	yarn knip

fmt:
	cd src-tauri && cargo fmt
