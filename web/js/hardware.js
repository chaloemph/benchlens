// Hardware view: hardware.json (and the run's own facts) of the selected runs side by side (GET /api/runs/{id}).

import { api } from './api.js';
import { store, sortedRuns, sideInfo } from './data.js';
import { setState } from './state.js';
import { getComparison } from './compare.js';
import { h, clear, stateBlock, errorBlock } from './ui.js';
import { archName, shortDate } from './format.js';

// Shared with the runs view (report.md link); dropped on every reload.
const detailCache = new Map();
export function getDetail(id) {
  if (!detailCache.has(id)) {
    const p = api.run(id);
    detailCache.set(id, p);
    p.catch(() => detailCache.delete(id));
  }
  return detailCache.get(id);
}
export function dropDetailCache() { detailCache.clear(); }

let pickerFor = null;
let seq = 0;

export function initHardware() {
  document.getElementById('hw-diff').addEventListener('change', () => window.dispatchEvent(new Event('statechange')));
}

function syncPicker(s) {
  const box = document.querySelector('#hw-pick .checks');
  if (pickerFor !== store.runs) {
    clear(box);
    for (const r of sortedRuns()) {
      const cb = h('input', { type: 'checkbox', value: r.run_id });
      cb.addEventListener('change', () => {
        const cur = new Set(currentRuns());
        if (cb.checked) cur.add(r.run_id); else cur.delete(r.run_id);
        const order = sortedRuns().map((x) => x.run_id);
        setState({ run: [...cur].sort((a, b) => order.indexOf(a) - order.indexOf(b)) });
      });
      const info = sideInfo(r.run_id);
      box.append(h('label', {}, cb, h('span', { text: info.label }), ' ', h('span', { class: 'sub', text: info.sub })));
    }
    pickerFor = store.runs;
  }
  for (const cb of box.querySelectorAll('input')) cb.checked = s.run.includes(cb.value);
  document.getElementById('hw-picked').textContent = `(${s.run.length} of ${store.runs.length})`;
  if (!s.run.length) document.getElementById('hw-pick').open = true;
}

let currentRuns = () => [];

/** Flatten nested JSON to dotted paths; arrays of objects use the item's name (or locator) when it has one. */
function flatten(v, path, out) {
  if (v === null || v === undefined) { out.set(path, '—'); return; }
  if (Array.isArray(v)) {
    if (!v.length) { out.set(path, '—'); return; }
    if (v.every((x) => x === null || typeof x !== 'object')) { out.set(path, v.map((x) => (x === null ? '—' : String(x))).join('; ')); return; }
    v.forEach((x, i) => {
      const key = x && typeof x === 'object' && (x.name || x.locator) ? String(x.name || x.locator) : String(i);
      flatten(x, `${path}[${key}]`, out);
    });
    return;
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    if (!keys.length) { out.set(path, '—'); return; }
    for (const k of keys) flatten(v[k], path ? path + '.' + k : k, out);
    return;
  }
  out.set(path, String(v));
}

function runFacts(d) {
  const m = new Map();
  m.set('label', d.label);
  m.set('folder', d.folder);
  m.set('run id', d.run_id);
  m.set('group', d.group);
  m.set('arch', archName(d.arch) + ' (' + d.arch + ')');
  m.set('vCPU', d.nproc == null ? '—' : String(d.nproc));
  m.set('RAM GB', d.ram_gb == null ? '—' : String(d.ram_gb));
  m.set('CPU model', d.cpu_model || '—');
  m.set('vmbench', d.vmbench_version);
  m.set('started', shortDate(d.start_utc));
  m.set('ended', shortDate(d.end_utc));
  m.set('stages', d.stages || '—');
  m.set('failed stages', d.failed_stages.length ? d.failed_stages.join(', ') : '—');
  m.set('PostgreSQL profile', d.pg_profile || '—');
  m.set('web CPUs', d.web_cpus || '—');
  m.set('tags', Object.entries(d.tags).map(([k, v]) => k + '=' + v).join('; ') || '—');
  return m;
}

export function renderHardware(s) {
  currentRuns = () => s.run;
  syncPicker(s);
  const my = ++seq;
  const fromCompare = document.getElementById('hw-from-compare');
  fromCompare.disabled = !(s.base && s.other.length);
  fromCompare.onclick = async () => {
    try {
      const c = await getComparison(s.base, s.other);
      setState({ run: [...new Set([c.base, ...c.others].flatMap((x) => x.run_ids))] });
    } catch (err) {
      clear(document.getElementById('hw-table'), h('tbody', {}, h('tr', {}, h('td', {}, errorBlock(err, 'Cannot load the comparison')))));
    }
  };
  const table = document.getElementById('hw-table');
  const msg = (node) => clear(table, h('tbody', {}, h('tr', {}, h('td', { class: 'left' }, node))));
  if (!s.run.length) { msg(stateBlock('empty', 'Pick runs above, select them in the Runs view, or take the runs of the current comparison.')); return; }
  msg(stateBlock('loading', 'Loading hardware…'));
  Promise.all(s.run.map(getDetail)).then((details) => {
    if (my !== seq) return;
    const onlyDiff = document.getElementById('hw-diff').checked;
    const sections = [['Run', details.map(runFacts)]];
    const hw = details.map((d) => { const m = new Map(); if (d.hardware) flatten(d.hardware, '', m); return m; });
    // group hardware paths by their top-level key, in first-seen order
    const tops = [];
    for (const m of hw) for (const k of m.keys()) { const t = k.split(/[.[]/)[0]; if (!tops.includes(t)) tops.push(t); }
    for (const t of tops) {
      if (t === 'schema') continue;
      sections.push([t, hw.map((m) => new Map([...m].filter(([k]) => k.split(/[.[]/)[0] === t)))]);
    }
    const head = h('tr', {}, h('th', { scope: 'col', text: 'Field' }),
      ...details.map((d) => h('th', { scope: 'col' }, h('div', { text: d.label }), h('div', { class: 'muted', text: d.group }))));
    const body = h('tbody');
    let shown = 0;
    for (const [name, maps] of sections) {
      const keys = [];
      for (const m of maps) for (const k of m.keys()) if (!keys.includes(k)) keys.push(k);
      const rows = [];
      for (const k of keys) {
        const vals = maps.map((m) => (m.has(k) ? m.get(k) : '—'));
        const differs = new Set(vals).size > 1;
        if (onlyDiff && !differs && !(name === 'Run' && k === 'label')) continue;
        rows.push(h('tr', { class: differs ? 'differs' : '' },
          h('th', { scope: 'row', text: k }),
          ...vals.map((v) => h('td', { text: v }))));
      }
      if (!rows.length) continue;
      body.append(h('tr', { class: 'group-row' }, h('th', { scope: 'rowgroup', colspan: String(details.length + 1), text: name === 'Run' ? 'Run' : 'hardware.json: ' + name })));
      body.append(...rows);
      shown += rows.length;
    }
    const noHw = details.filter((d) => !d.hardware).map((d) => d.label);
    if (noHw.length) body.append(h('tr', {}, h('td', { colspan: String(details.length + 1), class: 'left muted', text: 'No hardware.json for: ' + noHw.join(', ') })));
    if (!shown) body.append(h('tr', {}, h('td', { colspan: String(details.length + 1), class: 'left muted', text: 'Nothing differs.' })));
    clear(table, h('caption', { class: 'sr-only', text: 'Hardware of the selected runs; highlighted rows differ' }), h('thead', {}, head), body);
  }).catch((err) => {
    if (my !== seq) return;
    msg(errorBlock(err, 'Cannot load a run'));
  });
}
