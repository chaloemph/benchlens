//! benchlens-wasm: the verdict rule of [`benchlens_rules`] for the browser.
//!
//! The JavaScript side calls `compare(inputJson)` with
//! `{"base": {"side": Side, "values": [ValueRow]}, "others": [{"side": Side, "values": [ValueRow]}]}`
//! and gets back the `Comparison` JSON, the same shape as `GET /api/compare` (serde of
//! [`benchlens_rules::compare::Comparison`]). Build with `scripts/build-wasm.sh`.

use benchlens_rules::compare::{compare_sides, Side, ValueRow};
use serde::Deserialize;

/// One side of the input: who it is and every value that counts for it.
#[derive(Debug, Deserialize)]
pub struct SideInput {
    pub side: Side,
    pub values: Vec<ValueRow>,
}

/// What `compare` takes.
#[derive(Debug, Deserialize)]
pub struct CompareInput {
    pub base: SideInput,
    pub others: Vec<SideInput>,
}

/// Version of this crate (the benchlens workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parse the input, judge every side against the base and return the `Comparison` as JSON.
/// This is what the wasm `compare` export runs; it is plain Rust so native tests can call it.
pub fn compare_json(input_json: &str) -> Result<String, String> {
    let input: CompareInput =
        serde_json::from_str(input_json).map_err(|e| format!("benchlens compare input: {e}"))?;
    if input.others.is_empty() {
        return Err("nothing to compare the baseline with".into());
    }
    let c = compare_sides(
        (input.base.side, input.base.values),
        input
            .others
            .into_iter()
            .map(|s| (s.side, s.values))
            .collect(),
    );
    serde_json::to_string(&c).map_err(|e| format!("benchlens compare output: {e}"))
}

#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::*;

    /// `compare(inputJson: string): string` (the Comparison JSON); throws an Error on bad input.
    #[wasm_bindgen]
    pub fn compare(input_json: &str) -> Result<String, JsValue> {
        super::compare_json(input_json).map_err(|e| JsError::new(&e).into())
    }

    /// `version(): string`, the benchlens version this build came from.
    #[wasm_bindgen]
    pub fn version() -> String {
        super::VERSION.to_string()
    }
}
