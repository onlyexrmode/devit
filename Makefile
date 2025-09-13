.PHONY: fmt clippy test build smoke ci help

help:
	@echo "Targets: fmt | clippy | test | build | smoke | ci"

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test -p devit-common
	cargo test -p devit --tests

build:
	cargo build --workspace

smoke:
	./scripts/prepush-smoketest.sh

ci: fmt clippy build test

