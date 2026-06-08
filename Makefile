# Solana Yield Adapter Standard — developer Makefile
#
# Quick start:
#   make install            # install JS deps + check toolchain
#   make build              # anchor build (all programs)
#   make fork-up            # start surfpool mainnet fork (needs a mainnet RPC)
#   make test-fork          # run all mainnet-fork integration tests
#   make fork-down          # stop the fork
#
# Run `make` or `make help` to list every target.

# ── Shell / make behaviour ───────────────────────────────────────────────────
SHELL        := bash
.SHELLFLAGS  := -eu -o pipefail -c
.DEFAULT_GOAL := help
.ONESHELL:

# ── Configurable variables (override on the CLI: `make test-fork GREP=Drift`) ─
PORT        ?= 8899
RPC         ?= http://127.0.0.1:$(PORT)
WALLET      ?= $(HOME)/.config/solana/id.json
# Mainnet datasource the fork pulls accounts from. Public api.mainnet-beta does
# NOT work with surfpool; use a real RPC (e.g. Helius). Falls back to the env var.
DATASOURCE_RPC ?= $(SURFPOOL_DATASOURCE_RPC_URL)
# Single program for build-one / deploy-one (anchor program name, snake_case).
PROGRAM     ?=
# Mocha --grep filter for fork tests.
GREP        ?=
# Cluster for `make deploy` (localnet | devnet | mainnet).
CLUSTER     ?= devnet

# Adapter pubkey for registry-status.
ADAPTER     ?=

# Test invocation shared env (provider + wallet for Anchor/web3 clients).
FORK_ENV    := ANCHOR_PROVIDER_URL=$(RPC) ANCHOR_WALLET=$(WALLET)

SURF_LOG    := .surfpool/logs

# surfpool exposes JSON-RPC only — there is NO GET /health (it returns HTTP 405).
# Health must be probed with a POST getHealth call.
HEALTH_PROBE := curl -s -m 3 $(RPC) -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}'

# ─────────────────────────────────────────────────────────────────────────────
##@ Help

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage: make \033[36m<target>\033[0m\n"} \
		/^[a-zA-Z0-9_-]+:.*?##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } \
		/^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)
	@echo

# ─────────────────────────────────────────────────────────────────────────────
##@ Setup

.PHONY: install
install: check-tools ## Install JS dependencies (npm)
	npm install

.PHONY: check-tools
check-tools: ## Verify required toolchain is on PATH
	@missing=0
	for t in anchor cargo node npm surfpool; do
		if ! command -v $$t >/dev/null 2>&1; then echo "  ✗ missing: $$t"; missing=1; \
		else echo "  ✓ $$t: $$($$t --version 2>/dev/null | head -1)"; fi
	done
	[ $$missing -eq 0 ] || { echo "Install the missing tools above."; exit 1; }

# ─────────────────────────────────────────────────────────────────────────────
##@ Build

.PHONY: build
build: ## anchor build — compile all programs + generate IDLs
	anchor build --ignore-keys

.PHONY: build-one
build-one: ## anchor build a single PROGRAM=<name> (e.g. drift_adapter)
	@[ -n "$(PROGRAM)" ] || { echo "set PROGRAM=<name>, e.g. make build-one PROGRAM=drift_adapter"; exit 1; }
	anchor build --ignore-keys -p $(PROGRAM)

.PHONY: sbf
sbf: ## cargo build-sbf — fast program-only build (no IDL), PROGRAM optional
	@if [ -n "$(PROGRAM)" ]; then \
		cargo build-sbf --manifest-path programs/$(subst _,-,$(PROGRAM))/Cargo.toml; \
	else cargo build-sbf; fi

# ─────────────────────────────────────────────────────────────────────────────
##@ Lint & format

.PHONY: fmt
fmt: ## cargo fmt — format Rust
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## cargo fmt --check — fail if unformatted
	cargo fmt --all --check

.PHONY: clippy
clippy: ## cargo clippy — Rust linter (warnings = errors)
	cargo clippy --all-targets -- -D warnings

.PHONY: lint
lint: fmt-check clippy ## Run fmt-check + clippy

# ─────────────────────────────────────────────────────────────────────────────
##@ Test

.PHONY: test
test: ## anchor test — full suite (Anchor spins up its own validator)
	anchor test

.PHONY: test-fork
test-fork: require-fork ## Run all mainnet-fork tests (surfpool must be up)
	@$(FORK_ENV) npm run test:fork $(if $(GREP),-- --grep "$(GREP)",)

# Per-adapter fork shortcuts (filter by the suite's describe() name).
.PHONY: test-marginfi test-kamino test-drift test-jupiter test-maple test-dispatcher
test-marginfi:   ## Fork test: MarginFi adapter
	@$(MAKE) test-fork GREP=MarginFi
test-kamino:     ## Fork test: Kamino adapter
	@$(MAKE) test-fork GREP=Kamino
test-drift:      ## Fork test: Drift adapter
	@$(MAKE) test-fork GREP=Drift
test-jupiter:    ## Fork test: Jupiter LP adapter
	@$(MAKE) test-fork GREP=Jupiter
test-maple:      ## Fork test: Maple adapter
	@$(MAKE) test-fork GREP=Maple
test-dispatcher: ## Fork test: Dispatcher e2e
	@$(MAKE) test-fork GREP=Dispatcher

.PHONY: ci-fork
ci-fork: require-fork ## Fork tests in CI mode (a missing/broken fork fails instead of skipping)
	@FORK_REQUIRED=1 $(FORK_ENV) npm run test:fork

# ─────────────────────────────────────────────────────────────────────────────
##@ Mainnet fork (surfpool)

.PHONY: fork-up
fork-up: ## Start surfpool mainnet fork (needs a mainnet datasource RPC)
	@if $(HEALTH_PROBE) 2>/dev/null | grep -q '"result":"ok"'; then echo "fork already running on :$(PORT)"; exit 0; fi
	@[ -n "$(DATASOURCE_RPC)" ] || { \
		echo "ERROR: no mainnet datasource RPC set."; \
		echo "  export SURFPOOL_DATASOURCE_RPC_URL=https://mainnet.helius-rpc.com/?api-key=..."; \
		echo "  (or: make fork-up DATASOURCE_RPC=<url>)"; exit 1; }
	@echo "Starting surfpool fork (datasource: $$(echo '$(DATASOURCE_RPC)' | sed 's/api-key=.*/api-key=***/'))"
	@NO_DNA=1 SURFPOOL_DATASOURCE_RPC_URL="$(DATASOURCE_RPC)" \
		nohup surfpool start --no-tui -y --no-studio --port $(PORT) >/dev/null 2>&1 & disown
	@for i in $$(seq 1 40); do \
		if $(HEALTH_PROBE) 2>/dev/null | grep -q '"result":"ok"'; then echo "  ✓ fork ready on :$(PORT)"; exit 0; fi; \
		sleep 2; done; \
		echo "  ✗ fork did not become ready in 80s — check $(SURF_LOG)/"; exit 1

.PHONY: fork-down
fork-down: ## Stop the surfpool fork
	@pkill -f '[s]urfpool start' 2>/dev/null && echo "fork stopped" || echo "no fork running"

.PHONY: fork-status
fork-status: ## Check whether the fork RPC is responding
	@if $(HEALTH_PROBE) 2>/dev/null | grep -q '"result":"ok"'; then echo "up: $(RPC)"; else echo "down: $(RPC)"; fi

.PHONY: fork-logs
fork-logs: ## Tail the latest surfpool log
	@ls -t $(SURF_LOG)/*.log 2>/dev/null | head -1 | xargs -r tail -f

.PHONY: require-fork
require-fork: ## (internal) fail fast if the fork is not up
	@$(HEALTH_PROBE) 2>/dev/null | grep -q '"result":"ok"' || { \
		echo "Fork not running on $(RPC). Start it with: make fork-up"; exit 1; }

# ─────────────────────────────────────────────────────────────────────────────
##@ Deploy & registry

.PHONY: deploy
deploy: ## anchor deploy (CLUSTER=localnet|devnet|mainnet, default devnet)
	anchor deploy --provider.cluster $(CLUSTER)

.PHONY: registry-list
registry-list: ## List registered adapters (ANCHOR_PROVIDER_URL/WALLET aware)
	@$(FORK_ENV) npx tsx scripts/register-adapter.ts list

.PHONY: registry-status
registry-status: ## Show one adapter's RegistryEntry (ADAPTER=<pubkey>)
	@[ -n "$(ADAPTER)" ] || { echo "set ADAPTER=<pubkey>"; exit 1; }
	@$(FORK_ENV) npx tsx scripts/register-adapter.ts status --adapter $(ADAPTER)

# ─────────────────────────────────────────────────────────────────────────────
##@ Housekeeping

.PHONY: clean
clean: ## Remove build artifacts (anchor/cargo target, .anchor)
	anchor clean 2>/dev/null || true
	cargo clean 2>/dev/null || true
	rm -rf .anchor

.PHONY: clean-logs
clean-logs: ## Remove surfpool logs
	rm -rf $(SURF_LOG)/*.log

.PHONY: distclean
distclean: clean clean-logs ## clean + remove node_modules
	rm -rf node_modules
