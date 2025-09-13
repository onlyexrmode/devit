.PHONY: fmt fmt-check fmt-fix clippy lint test test-cli build build-release smoke ci check verify help

help:
	@echo "Targets: fmt | fmt-check | fmt-fix | clippy | lint | test | test-cli | build | build-release | smoke | check | verify | ci"

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

fmt-fix: fmt

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

lint: clippy fmt-check

test:
	cargo test -p devit-common
	cargo test -p devit --tests

test-cli:
	cargo test -p devit --tests

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

smoke:
	./scripts/prepush-smoketest.sh

plan:
	cargo run -p devit -- plan

watch:
	cargo run -p devit -- watch

check: fmt-check clippy

verify: check build test

ci: verify
