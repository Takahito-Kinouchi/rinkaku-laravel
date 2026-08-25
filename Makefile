.PHONY: test lint audit format help

test: ## Run all tests
	cargo test --all-features

lint: ## Check formatting and run clippy with warnings as errors
	cargo fmt --all --check
	cargo clippy --all-targets --all-features -- -D warnings

audit: ## Check dependencies against the RustSec advisory database
	@command -v cargo-audit >/dev/null 2>&1 || { \
		echo "cargo-audit not found; install it with: cargo install cargo-audit --locked"; \
		exit 1; \
	}
	cargo audit

format: ## Auto-format the codebase
	cargo fmt --all

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-10s\033[0m %s\n", $$1, $$2}'
