# Wave 1 review: E (WebAssembly build of the verdict rule)

Branch `m2/e-wasm` at bb5efba. Verdict: **ok**.

## What was checked

- `crates/wasm` only parses JSON, calls `benchlens_rules::compare::compare_sides` and serialises the result;
  the verdict rule is not touched. wasm-bindgen is a target-only dependency, so `cargo test --all` builds it
  natively.
- `scripts/build-wasm.sh` on rustc 1.100.0 with wasm-bindgen-cli 0.2.130 rebuilds `web/wasm/` byte for byte
  (no diff), also after F's `[profile.release]` was merged (`[profile.wasm]` sets every option it relies on).
- `scripts/test-wasm.sh`: 336 golden verdicts in 6 pairs through the wasm build in node; each verdict, diff
  and base median against vmbench's `report compare`, the row count, and byte equality with the native
  Comparison JSON. Passed after every merge.
- `serde_json` `float_roundtrip`, enabled workspace-wide: **right**. Without it, serde_json's fast float
  parser can be one ULP off, and the wasm path (values go through JSON) differs from native: I rebuilt
  without the feature and `test-wasm.sh` failed on `x86-high-r2 vs arm64-high-r2` (byte equality). With it,
  parsing is correctly rounded everywhere; the only cost is a slightly slower float parse. The golden tests
  pass with it, so no verdict moved.
- CI: caches the wasm-bindgen binary, rebuilds, warns (does not fail) when the committed files differ from a
  fresh build because the bytes depend on rustc. Reasonable.

## Notes

- `crates/wasm` needs Rust 1.81 (wasm-bindgen 0.2.130), the rest of the workspace 1.80. `cargo install --path
  crates/cli` still works on 1.80; `cargo test --all` needs 1.81.
- The web UI does not load the wasm yet; task D (export) is its first user.
