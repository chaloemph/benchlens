// Every verdict, diff and base median of tests/golden/compare.json (vmbench's own report compare), computed
// by the WebAssembly build in web/wasm/. Same tolerance as crates/core/tests/golden.rs. Also checks that the
// wasm output is byte for byte the native benchlens Comparison JSON (GET /api/compare).
//
//   node tests/wasm/golden.mjs INPUTS.json   (INPUTS.json from: cargo run -p benchlens-wasm --example golden_inputs)
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import init, { compare, version } from "../../web/wasm/benchlens_wasm.js";

const root = new URL("../../", import.meta.url);
const wasm = readFileSync(fileURLToPath(new URL("web/wasm/benchlens_wasm_bg.wasm", root)));
await init({ module_or_path: wasm });

const golden = JSON.parse(readFileSync(fileURLToPath(new URL("tests/golden/compare.json", root)), "utf8"));
const inputs = JSON.parse(readFileSync(process.argv[2], "utf8"));
const VERDICT = { better: 1, worse: -1, not_clear: 0 };

const pairs = new Map();
for (const r of golden.rows) {
  const k = `${r.base}\u0000${r.other}`;
  if (!pairs.has(k)) pairs.set(k, []);
  pairs.get(k).push(r);
}
const cases = new Map(inputs.cases.map((c) => [`${c.base}\u0000${c.other}`, c]));
assert.equal(cases.size, pairs.size, "one input per golden pair");

let checked = 0;
for (const [k, rows] of pairs) {
  const [base, other] = k.split("\u0000");
  const c = cases.get(k);
  assert.ok(c, `${base} vs ${other}: no input`);
  const out = compare(JSON.stringify(c.input));
  assert.equal(out, c.native, `${base} vs ${other}: wasm output differs from native benchlens`);
  const cmp = JSON.parse(out);
  const ours = new Map(cmp.rows.map((r) => [r.id, r]));
  const judged = rows.filter((g) => g.verdict !== null);
  for (const g of judged) {
    const row = ours.get(g.id);
    assert.ok(row, `${base} vs ${other}: ${g.id} missing`);
    assert.equal(VERDICT[row.verdict], g.verdict, `${base} vs ${other}: ${g.id} verdict`);
    if (g.diff === null || row.diff === null) {
      assert.equal(row.diff, g.diff, `${g.id}: diff`);
    } else {
      assert.ok(Math.abs(g.diff - row.diff) < 1e-9 * Math.max(Math.abs(g.diff), 1), `${g.id}: diff ${g.diff} vs ${row.diff}`);
    }
    assert.ok(
      Math.abs(g.median_base - row.base) < 1e-9 * Math.max(Math.abs(row.base), 1),
      `${g.id}: base median ${g.median_base} vs ${row.base}`,
    );
    checked++;
  }
  assert.equal(cmp.rows.length, judged.length, `${base} vs ${other}: row count`);
}
assert.ok(checked > 300, `only ${checked} verdicts checked`);
assert.throws(() => compare("{"), /benchlens compare input/);
console.log(`benchlens-wasm ${version()}: ${checked} verdicts in ${pairs.size} pairs match vmbench report compare`);
