IDL_OUT_DIR := "idl-out"
FEATURES := "cli,u128"
SCRIPTS := "./scripts"
TARGET := "./target"

RESOURCES := SCRIPTS / "resources"
CONFIGS := RESOURCES / "config"

GEYSER_PLUGIN_PATH := RESOURCES  / "geyser/plugin.geyser"
START_LOCALNET_SCRIPT := SCRIPTS / "start_localnet.sh"
SETUP_LOCALNET_SCRIPT := SCRIPTS / "setup_localnet.sh"

export GMSOL_TOKENS := CONFIGS / "tokens.toml"
export GMSOL_MARKETS := CONFIGS / "markets.toml"
export GMSOL_MARKET_CONFIGS := CONFIGS / "market_configs.toml"
LOCALNET_USDG_KEYPAIR := RESOURCES / "keypair" / "usdg.json"

VERIFIABLE := TARGET / "verifiable"
STORE_PROGRAM := VERIFIABLE / "gmsol_store.so"
TREASURY_PROGRAM := VERIFIABLE / "gmsol_treasury.so"
MOCK_CHAINLINK_PROGRAM := VERIFIABLE / "mock_chainlink_verifier.so"

default: lint test-crates test-programs

lint: && build-docs
  cargo fmt --check
  cargo clippy --features {{FEATURES}}

test: test-crates test-programs

test-crates:
  cargo test --features {{FEATURES}}

test-programs *ARGS:
  anchor test {{ARGS}}

test-programs-debug *ARGS:
  anchor test {{ARGS}} -- --features debug-msg

build-docs *ARGS:
  cargo doc --features doc {{ARGS}}

build-idls:
  mkdir -p {{IDL_OUT_DIR}}
  anchor idl build -p gmsol_store -t {{IDL_OUT_DIR}}/gmsol_store.ts -o {{IDL_OUT_DIR}}/gmsol_store.json

check-verifiable:
  @if [ -f {{STORE_PROGRAM}} ] && [ -f {{TREASURY_PROGRAM}} ] && [ -f {{MOCK_CHAINLINK_PROGRAM}} ]; then \
    echo "Verifiable programs found."; \
  else \
    echo "Verifiable programs not found. Please build them."; \
    exit 1; \
  fi

build-verifiable:
  anchor build -v -- --features no-mock

build-verifiable-with-mock:
  anchor build -v

check-geyser:
  @if [ -f "{{GEYSER_PLUGIN_PATH}}" ]; then \
    echo "Geyser plugin found: {{GEYSER_PLUGIN_PATH}}"; \
  else \
    echo "Geyser plugin not found. Please build the plugin."; \
    exit 1; \
  fi

start-localnet: check-geyser check-verifiable
  sh {{START_LOCALNET_SCRIPT}}

setup-localnet keeper oracle="42":
  @GMSOL_KEEPER={{absolute_path(keeper)}} \
  GMSOL_ORACLE_SEED={{oracle}} \
  LOCALNET_USDG_KEYPAIR={{absolute_path(LOCALNET_USDG_KEYPAIR)}} \
  sh {{SETUP_LOCALNET_SCRIPT}}
