.PHONY: dev build test test-rust test-ui lint

dev:
	yarn tauri dev

build:
	yarn tauri build

test: test-rust test-ui

test-rust:
	cd src-tauri && cargo test

test-ui:
	yarn vitest run

lint:
	cd src-tauri && cargo clippy -- -D warnings
	yarn tsc --noEmit
