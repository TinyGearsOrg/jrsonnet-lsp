# Common tasks for the language server and the VS Code extension.
# `make` lists the targets.

EXT := extension

# What `make init` sets up. Targets that need them depend on these files, so they also work on a
# checkout where `make init` was never run (or ran before the dependencies changed).
JRSONNET := .jrsonnet/.stamp
NPM := $(EXT)/node_modules/.package-lock.json

.DEFAULT_GOAL := help
.PHONY: help init build build-server build-extension test test-server test-extension \
	test-integration lint fmt gen-stdlib bundle package clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | while IFS= read -r line; do \
		printf '  %-16s %s\n' "$${line%%:*}" "$${line##*## }"; \
	done

$(JRSONNET): scripts/setup-jrsonnet.sh Cargo.toml $(wildcard patches/*.patch)
	./scripts/setup-jrsonnet.sh
	@touch $@

$(NPM): $(EXT)/package.json $(EXT)/package-lock.json
	cd $(EXT) && npm ci
	@touch $@

init: $(JRSONNET) $(NPM) ## Prepare a fresh checkout: patched jrsonnet sources and the extension's npm packages

build: build-server build-extension ## Build the server (release) and the extension

build-server: $(JRSONNET) ## Build the language server in release mode
	cargo build --release

build-extension: $(NPM) ## Bundle the extension's TypeScript
	cd $(EXT) && npm run compile

test: test-server test-extension ## Run all tests

test-server: $(JRSONNET) ## Run the server's unit and end-to-end tests
	cargo test

test-extension: $(NPM) ## Type-check the extension and run its tests
	cd $(EXT) && npm run typecheck && npm test

test-integration: build-server build-extension ## Run the extension in a real VS Code (downloads VS Code; needs a display, e.g. xvfb-run)
	cd $(EXT) && npm run test:integration

lint: $(JRSONNET) $(NPM) ## Check formatting and run clippy, treating warnings as errors
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cd $(EXT) && npm run typecheck

fmt: ## Format the Rust code
	cargo fmt

gen-stdlib: ## Regenerate the `std` table from jrsonnet's own evaluator
	./scripts/gen-stdlib.sh

bundle: build-server build-extension ## Copy the release server into the extension for this platform
	cd $(EXT) && npm run bundle-server

package: bundle ## Build a local VSIX for this platform (releases use a static musl binary on Linux, see release.yml)
	cd $(EXT) && npm run package

clean: ## Remove build outputs (keeps .jrsonnet and node_modules)
	cargo clean
	rm -rf $(EXT)/out $(EXT)/server $(EXT)/*.vsix
