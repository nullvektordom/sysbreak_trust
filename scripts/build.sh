#!/usr/bin/env bash
set -euo pipefail

# Build all CosmWasm contracts for deployment
# Without Docker, we compile locally with wasm32-unknown-unknown target
# and optimize with wasm-opt if available.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(dirname "$SCRIPT_DIR")"
ARTIFACTS_DIR="$WORKSPACE_DIR/artifacts"

mkdir -p "$ARTIFACTS_DIR"

echo "==> Building all contracts in release mode..."
cargo build \
    --release \
    --target wasm32-unknown-unknown \
    --manifest-path "$WORKSPACE_DIR/Cargo.toml"

WASM_DIR="$WORKSPACE_DIR/target/wasm32-unknown-unknown/release"

CONTRACTS=(
    "sysbreak_item_nft"
    "sysbreak_achievement_nft"
    "sysbreak_credit_bridge"
    "sysbreak_corporation_dao"
)

for contract in "${CONTRACTS[@]}"; do
    wasm_file="$WASM_DIR/${contract}.wasm"
    if [ ! -f "$wasm_file" ]; then
        echo "WARNING: $wasm_file not found, skipping"
        continue
    fi

    out_file="$ARTIFACTS_DIR/${contract}.wasm"
    if command -v wasm-opt &> /dev/null; then
        echo "==> Optimizing ${contract}.wasm with wasm-opt..."
        wasm-opt -Oz --signext-lowering "$wasm_file" -o "$out_file"
    else
        echo "==> wasm-opt not found, copying unoptimized ${contract}.wasm"
        cp "$wasm_file" "$out_file"
    fi

    size=$(stat --printf="%s" "$out_file" 2>/dev/null || stat -f "%z" "$out_file" 2>/dev/null)
    echo "    ${contract}.wasm: ${size} bytes"
done

echo "==> Build complete. Artifacts in: $ARTIFACTS_DIR"
