# supervictor — make targets over ./Quickstart. `make help` lists them.
#
# `make` alone is the whole guided path (toolchain → certs → build → tests →
# local mTLS endpoint → a simulated machine or a board), the same as
# `./Quickstart`. Each stage is also its own target. Variables you can pass:
#   QS_HARDWARE=none|sim|flash  QS_PORT=8443  QS_TESTS=0  SIM_PROFILE=extruder
# `make up` runs them, so `make up QS_HARDWARE=none` is the non-interactive form.

QS := ./Quickstart

.PHONY: help up down status install-dev lint fmt build test-unit test-integration test-all \
        flash onboard certs-ca certs-device certs-admin certs-server fleet ping deploy release-check mcp

.DEFAULT_GOAL := up

help:  ## Show this help.
	@awk 'BEGIN{FS=":.*##"} /^[a-zA-Z_-]+:.*##/ { printf "  \033[1m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

# ---- Guided path -------------------------------------------------------------

up:  ## Start to finish: toolchain, .env, certs, build, tests, local endpoint, telemetry.
	$(QS) up

down:  ## Stop the local endpoint and simulator.
	$(QS) down

status:  ## What the local endpoint and simulator are doing.
	$(QS) status

# ---- Stages --------------------------------------------------------------------

install-dev:  ## Toolchains (esp via espup) + fetch deps.
	$(QS) install

fmt:  ## cargo fmt across the workspace.
	cargo fmt --all

lint:  ## fmt --check + clippy --all-targets -D warnings.
	$(QS) lint

build:  ## Workspace (host) + every embedded target + the desktop client.
	$(QS) build

test-unit:  ## Full workspace test suite on the host.
	$(QS) unit

test-integration:  ## SAM-local roundtrips and the staging smoke (both skip when unset).
	$(QS) integration

test-all:  ## lint → build → unit → integration.
	$(QS) --test

# ---- Devices -------------------------------------------------------------------

flash:  ## Build + flash the ESP32-POE-ISO on ESPFLASH_PORT.
	$(QS) edge

onboard:  ## Cert → register → flash → verify. DEVICE=<name> OWNER=<id> HOST=<endpoint>.
	$(QS) onboard $(DEVICE) $(OWNER) $(HOST)

certs-ca:  ## Issue the fleet CA (once).
	$(QS) certs ca

certs-device:  ## Issue a device certificate. DEVICE=<name>.
	$(QS) certs device $(DEVICE)

certs-admin:  ## Issue a dashboard admin certificate + browser bundle. ADMIN=<name>.
	$(QS) certs admin $(ADMIN)

certs-server:  ## Issue a server certificate. NAME=<dir> HOST=<san>.
	$(QS) certs server $(NAME) $(HOST)

fleet:  ## Fleet health table over admin mTLS. HOST=<endpoint> PORT=<port>.
	$(QS) fleet $(HOST) $(PORT)

ping:  ## mTLS GET / with the device certificate. HOST=<endpoint> PORT=<port>.
	$(QS) ping $(HOST) $(PORT)

# ---- Deploy --------------------------------------------------------------------

deploy:  ## Deploy the self-hosted staging box (scripts/deploy-staging.sh).
	$(QS) deploy

release-check:  ## Release build of the endpoint for this host (a v* tag builds the static musl binaries in CI).
	cargo build -p supervictor-endpoint --release --locked

# ---- Agents ------------------------------------------------------------------

mcp:  ## Build the MCP status server (.mcp.json launches it via scripts/mcp.sh).
	cargo build -p supervictor-mcp --release
