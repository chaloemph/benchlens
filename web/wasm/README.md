# web/wasm: the verdict rule in the browser

Generated files, committed so the web UI and the static HTML export can load them without a Rust toolchain.
Do not edit them by hand; rebuild with `scripts/build-wasm.sh` after changing `crates/rules` or `crates/wasm`.

| File | |
|---|---|
| `benchlens_wasm.js` | ES module (`wasm-bindgen --target web`), no bundler needed |
| `benchlens_wasm_bg.wasm` | `crates/wasm` (`benchlens-wasm`) built for `wasm32-unknown-unknown` |

## Use

```js
import init, { compare, version } from "./wasm/benchlens_wasm.js";
await init(); // fetches benchlens_wasm_bg.wasm next to the module; or init({ module_or_path: bytes })
const comparison = JSON.parse(compare(JSON.stringify({
  base:   { side: { reference, label, run_ids }, values: [/* ValueRow */] },
  others: [{ side: { reference, label, run_ids }, values: [/* ValueRow */] }],
})));
```

`compare` returns the same JSON as `GET /api/compare` (see docs/api.md) and throws an `Error` on bad input
or when `others` is empty. `ValueRow` is the shape of `GET /api/values`. `version()` is the benchlens version.

## How they were built

- `scripts/build-wasm.sh`: `cargo build -p benchlens-wasm --lib --target wasm32-unknown-unknown --profile wasm`
  (opt-level "z", LTO, one codegen unit), then
  `wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section`.
  wasm-opt is run only when it is on PATH (it was not for the committed files).
- rustc 1.100.0, wasm-bindgen crate and wasm-bindgen-cli 0.2.130 (pinned with `=` in crates/wasm/Cargo.toml;
  the script refuses another CLI version: `cargo install wasm-bindgen-cli --version 0.2.130 --locked`).
- Size: `benchlens_wasm_bg.wasm` about 181 KB (72 KB gzipped).

## Tested

`scripts/test-wasm.sh` runs every pair of `tests/golden/compare.json` (vmbench's own `report compare`)
through these files in node and checks each verdict, diff and base median, and that the output is byte for
byte the native benchlens Comparison JSON. CI rebuilds, runs it on the fresh build and warns when the committed files differ (the bytes depend on the rustc version).
