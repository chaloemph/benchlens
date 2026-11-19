#!/usr/bin/env bash
# Run every pair of tests/golden/compare.json through the WebAssembly build in web/wasm/ with node.
# The inputs come from the fixtures imported with benchlens-core (crates/wasm/examples/golden_inputs.rs).
# Run scripts/build-wasm.sh first (or pass --build).
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${1:-}" = "--build" ]; then scripts/build-wasm.sh; fi
inputs=target/wasm-golden-inputs.json
cargo run -q -p benchlens-wasm --example golden_inputs -- "$inputs"
node tests/wasm/golden.mjs "$inputs"
