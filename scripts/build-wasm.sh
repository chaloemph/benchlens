#!/usr/bin/env bash
# Build crates/wasm for the browser into web/wasm/ (benchlens_wasm.js + benchlens_wasm_bg.wasm, an ES module
# for `wasm-bindgen --target web`, no bundler needed). Needs the wasm32-unknown-unknown target and
# wasm-bindgen-cli at the same version as the wasm-bindgen crate pinned in crates/wasm/Cargo.toml.
# wasm-opt (binaryen) is used when it is on PATH; it is optional.
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' crates/wasm/Cargo.toml)
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$want" ]; then
  echo "need wasm-bindgen-cli $want (found: ${have:-none}): cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

# Panic messages carry source paths; map the cargo registry to a fixed prefix so the bytes do not depend on
# where cargo lives (the committed files then match a CI build and carry no home directory).
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$cargo_home/registry/src=/cargo/registry/src"
cargo build -p benchlens-wasm --lib --target wasm32-unknown-unknown --profile wasm
out=web/wasm
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section --out-dir "$out" \
  target/wasm32-unknown-unknown/wasm/benchlens_wasm.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$out/benchlens_wasm_bg.wasm" "$out/benchlens_wasm_bg.wasm"
fi
ls -l "$out"/benchlens_wasm.js "$out"/benchlens_wasm_bg.wasm
