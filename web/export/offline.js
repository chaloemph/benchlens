// Entry point of the static HTML export (`benchlens export html`, GET /api/export.html).
//
// The exported file inlines web/index.html, the CSS, d3, every module of web/js/ (bundled by the Rust export
// code, crates/server/src/export.rs), the WASM build of the verdict rule (base64) and the data of the exported
// sides as JSON. This module replaces the network calls of `api` with that data: comparisons are computed by
// the WASM `compare`, which gives the same Comparison JSON as `benchlens compare --json`, then the same views
// as the live UI render it. Nothing is fetched; the page's Content-Security-Policy forbids it anyway.

import init, { compare as wasmCompare } from '../wasm/benchlens_wasm.js';
import { api, ApiError } from '../js/api.js';
import { start } from '../js/main.js';
import { h, clear, stateBlock } from '../js/ui.js';
import { shortDate } from '../js/format.js';

const readOnly = () => Promise.reject(new ApiError(400, 'This is a static export: nothing can be changed here.'));

function readData() {
  const el = document.getElementById('benchlens-data');
  return JSON.parse(el.textContent);
}

function wasmBytes() {
  const b64 = document.getElementById('benchlens-wasm').textContent.trim();
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

/** `{side, values}` for a reference: an exported side, or one run of an exported side (its values only). */
function makeResolver(data) {
  const runs = new Map(data.runs.map((r) => [r.run_id, r]));
  const bySide = new Map();
  for (const s of data.sides) if (!bySide.has(s.side.reference)) bySide.set(s.side.reference, s);
  return (ref) => {
    const s = bySide.get(ref);
    if (s) return { side: { reference: s.side.reference, label: s.side.label, run_ids: s.side.run_ids }, values: s.values };
    const run = runs.get(ref);
    if (run) {
      const from = data.sides.find((x) => x.side.run_ids.includes(ref));
      const values = from ? from.values.filter((v) => v.run_id === ref) : [];
      return { side: { reference: ref, label: run.label, run_ids: [ref] }, values };
    }
    throw new ApiError(404, `${ref} is not in this export (exported: ${[...bySide.keys()].join(', ')})`);
  };
}

function installApi(data) {
  const resolve = makeResolver(data);
  const runs = data.runs.map((r) => {
    const { stages, end_utc, imported_at, pg_profile, web_cpus, meta, hardware, files, ...summary } = r;
    return summary;
  });
  const asyncly = (f) => (...args) => new Promise((ok) => ok(f(...args)));
  Object.assign(api, {
    health: asyncly(() => ({ benchlens: data.benchlens, db_layout: null, runs: runs.length })),
    runs: asyncly(() => runs),
    groups: asyncly(() => data.groups),
    run: asyncly((id) => {
      const r = data.runs.find((x) => x.run_id === id);
      if (!r) throw new ApiError(404, `run ${id} is not in this export`);
      return r;
    }),
    compare: asyncly((base, others) => {
      if (!others.length) throw new ApiError(400, 'nothing to compare the baseline with');
      const input = { base: resolve(base), others: others.map(resolve) };
      return JSON.parse(wasmCompare(JSON.stringify(input)));
    }),
    values: asyncly((ref, id) => {
      const { side, values } = resolve(ref);
      return { side: { ...side, archs: [] }, id, values: values.filter((v) => v.id === id) };
    }),
    metrics: () => Promise.reject(new ApiError(404, 'metric tables are not part of a static export')),
    fileUrl: () => '#',
    patchTags: readOnly,
    deleteRun: readOnly,
    exportHtml: null,
  });
}

function banner(data) {
  const sides = [data.base, ...data.others].length;
  const runs = data.runs.length;
  const el = h('section', { class: 'card export-meta', 'aria-label': 'About this export' },
    h('p', {}, h('b', { text: data.title })),
    h('p', { class: 'muted' },
      `Static export of benchlens ${data.benchlens}, made ${shortDate(data.exported_at)} from ${data.source}: `
      + `${sides} side${sides === 1 ? '' : 's'}, ${runs} run${runs === 1 ? '' : 's'}. `
      + 'Baseline and compared sides can be changed among the exported ones; the verdicts are computed here by '
      + 'the same rule as benchlens compare (WebAssembly build of benchlens ' + data.benchlens + ').'),
    h('p', { class: 'muted', text: 'Exported comparison: ' + data.base + ' vs ' + data.others.join(', ') }));
  document.getElementById('main').prepend(el);
}

async function boot() {
  const status = document.getElementById('status');
  let data;
  try {
    data = readData();
    await init({ module_or_path: wasmBytes() });
  } catch (err) {
    clear(status, stateBlock('error', h('strong', { text: 'Cannot open this export: ' }), String(err && err.message ? err.message : err)));
    return;
  }
  installApi(data);
  banner(data);
  await start({
    views: ['compare', 'metric', 'hardware'],
    firstComparison: () => ({ base: data.base, others: data.others }),
    title: data.title,
  });
}

boot();
