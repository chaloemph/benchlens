// Compare view: baseline + compared sides, quick picks, takeaway, tally table, heatmap.
// Every number comes from GET /api/compare as it is; nothing is recomputed here.

import { api } from './api.js';
import { store, sideInfo, sortedGroups, sortedRuns, groupRef, quickPicks, splitKeys, splitValues, splitRef } from './data.js';
import { setState } from './state.js';
import { h, clear, stateBlock, errorBlock, showTip, hideTip } from './ui.js';
import { archName, fmt, pct, pct1, sectionName, sortSections, VERDICT_TEXT } from './format.js';

// Key metrics (from the prototype): shown first; missing ones are skipped.
export const KEY_METRICS = ['cpu.7z.rating.max', 'cpu.cpubench.int.compress.1t', 'cpu.cpubench.int.compress.max', 'cpu.cpubench.int.json.max',
  'cpu.cpubench.float.matmul.max', 'cpu.openssl.aes-256-gcm.max', 'cpu.cpubench.crypto.aes-gcm.max', 'mem.stream.triad.max', 'mem.stream.copy.max',
  'mem.lmbench.lat-mem-rd.1t', 'disk.fio.randread-4k.qd32x4', 'disk.fio.randwrite-4k.qd32x4', 'disk.fio.randread-4k.qd1.p99', 'disk.fio.seq-write-1m',
  'disk.wal.fdatasync.avg', 'disk.wal.commit-iops.qd1', 'pg.pgbench.tpcb.tps', 'pg.pgbench.tpcb.stability', 'pg.pgbench.latency.single',
  'net.iperf3.tcp.bidir', 'net.iperf3.tcp.tx', 'net.sockperf.pingpong.avg', 'web.cpubench.mix.max-rps'];

// ---- comparison cache (shared with the metric view) ----
let cache = { key: '', promise: null };
export function getComparison(base, others) {
  const key = base + '\n' + others.join('\n');
  if (cache.key !== key || !cache.promise) {
    const promise = api.compare(base, others);
    cache = { key, promise };
    promise.catch(() => { if (cache.promise === promise) cache = { key: '', promise: null }; });
  }
  return cache.promise;
}
export function dropComparisonCache() { cache = { key: '', promise: null }; }

// ---- picker ----
let pickerBuiltFor = null;

function sideLabel(info) {
  return [h('span', { text: info.label }), ' ', h('span', { class: 'sub', text: info.sub }),
    info.mixed ? h('span', { class: 'flag', text: 'mixes machines', title: 'This group has runs on different architectures, vCPU counts or CPU models' }) : null];
}

function buildPicker() {
  const base = document.getElementById('base');
  clear(base);
  const og1 = h('optgroup', { label: 'Groups (every run pooled)' });
  for (const g of sortedGroups()) {
    const i = sideInfo(groupRef(g));
    og1.append(h('option', { value: i.ref, text: i.label + ' (' + i.sub + ')' + (i.mixed ? ' · mixes machines' : '') }));
  }
  const og2 = h('optgroup', { label: 'Single runs' });
  for (const r of sortedRuns()) {
    const i = sideInfo(r.run_id);
    og2.append(h('option', { value: i.ref, text: i.label + ' (' + i.sub + ')' }));
  }
  base.append(og1, og2);
  base.onchange = () => {
    const s = currentState();
    setState({ base: base.value, other: s.other.filter((o) => o !== base.value) });
  };

  const fill = (box, refs) => {
    const checks = clear(box.querySelector('.checks'));
    for (const ref of refs) {
      const info = sideInfo(ref);
      const cb = h('input', { type: 'checkbox', value: ref });
      cb.addEventListener('change', () => {
        const s = currentState();
        const cur = s.other.filter((o) => o !== ref);
        setState({ other: inPickerOrder(cb.checked ? [...cur, ref] : cur) });
      });
      checks.append(h('label', {}, cb, ...sideLabel(info)));
    }
  };
  fill(document.getElementById('pick-groups'), sortedGroups().map(groupRef));
  fill(document.getElementById('pick-runs'), sortedRuns().map((r) => r.run_id));

  const presets = clear(document.getElementById('presets'), h('span', { class: 'lbl', text: 'Quick picks' }));
  for (const p of quickPicks()) {
    const b = h('button', { type: 'button', text: p.label, 'aria-pressed': 'false' });
    b.dataset.base = p.base;
    b.dataset.others = JSON.stringify(p.others);
    b.addEventListener('click', () => setState({ base: p.base, other: p.others }));
    presets.append(b);
  }
  if (presets.children.length === 1) presets.hidden = true;
  buildSplit();
  pickerBuiltFor = store.runs;
}

const allRefs = () => [...sortedGroups().map(groupRef), ...sortedRuns().map((r) => r.run_id)];

/** Listed sides in picker order, then the others (tag splits, sides from a link) in the order they were added. */
function inPickerOrder(refs) {
  const order = allRefs();
  const at = (r) => { const i = order.indexOf(r); return i < 0 ? Infinity : i; };
  return [...refs].sort((a, b) => at(a) - at(b));
}

// ---- split a group by tag: one side group:NAME@KEY=VALUE per chosen value ----
const split = { group: '', key: '', values: new Set() };

/** The split control, made once below the single runs (index.html stays as it is). */
function splitBox() {
  let box = document.getElementById('pick-split');
  if (box) return box;
  box = h('details', { class: 'picks-split', id: 'pick-split' },
    h('summary', {}, 'Compare with: a group split by tag ', h('span', { class: 'muted', id: 'split-picked' })),
    h('fieldset', { class: 'picks' },
      h('legend', { class: 'sr-only', text: 'Split a group by tag' }),
      h('p', { class: 'muted hint', text: 'One side per tag value: the runs of the group with that value, pooled (for example every run of round 1 against round 2).' }),
      h('div', { class: 'split-fields' },
        h('div', { class: 'field' }, h('label', { for: 'split-group', text: 'Group' }), h('select', { id: 'split-group' })),
        h('div', { class: 'field' }, h('label', { for: 'split-key', text: 'Tag' }), h('select', { id: 'split-key' }))),
      h('div', { class: 'checks', id: 'split-values', role: 'group', 'aria-label': 'Tag values' }),
      h('div', { class: 'split-actions' },
        h('label', { class: 'inline' }, h('input', { type: 'checkbox', id: 'split-as-base' }), 'First value as the baseline'),
        h('button', { type: 'button', id: 'split-add', text: 'Add sides' }),
        h('span', { class: 'muted', id: 'split-msg', role: 'status' })),
      h('div', { id: 'split-current' })));
  document.querySelector('#view-compare details.picks-runs').after(box);
  document.getElementById('split-group').addEventListener('change', (e) => { split.group = e.target.value; split.key = ''; split.values.clear(); fillSplit(); });
  document.getElementById('split-key').addEventListener('change', (e) => { split.key = e.target.value; split.values.clear(); fillSplit(); });
  document.getElementById('split-add').addEventListener('click', addSplitSides);
  return box;
}

function buildSplit() {
  splitBox();
  fillSplit();
}

/** Group, tag and value controls from the runs' tags; keeps what was chosen while it still exists. */
function fillSplit() {
  const groups = sortedGroups().map((g) => g.group).filter((g) => splitKeys(g).length);
  const gsel = document.getElementById('split-group');
  if (!groups.includes(split.group)) { split.group = groups[0] || ''; split.key = ''; split.values.clear(); }
  clear(gsel, ...groups.map((g) => h('option', { value: g, text: g })));
  gsel.value = split.group;
  gsel.disabled = !groups.length;

  const keys = split.group ? splitKeys(split.group) : [];
  const ksel = document.getElementById('split-key');
  if (!keys.includes(split.key)) { split.key = keys.includes('round') ? 'round' : keys[0] || ''; split.values.clear(); }
  clear(ksel, ...keys.map((k) => h('option', { value: k, text: k })));
  ksel.value = split.key;
  ksel.disabled = !keys.length;

  const vals = split.group && split.key ? splitValues(split.group, split.key) : [];
  for (const v of [...split.values]) if (!vals.some((x) => x.value === v)) split.values.delete(v);
  const box = clear(document.getElementById('split-values'));
  if (!vals.length) box.append(h('span', { class: 'muted', text: groups.length ? 'No value to split by.' : 'No group has tags to split by.' }));
  for (const { value, runs } of vals) {
    const cb = h('input', { type: 'checkbox', value, checked: split.values.has(value) });
    cb.addEventListener('change', () => { if (cb.checked) split.values.add(value); else split.values.delete(value); syncSplitButton(); });
    box.append(h('label', {}, cb, h('span', { text: split.key + '=' + value }), ' ',
      h('span', { class: 'sub', text: runs + (runs === 1 ? ' run' : ' runs') })));
  }
  syncSplitButton();
}

function chosenSplitRefs() {
  const order = splitValues(split.group, split.key).map((x) => x.value);
  return order.filter((v) => split.values.has(v)).map((v) => splitRef(split.group, split.key, v));
}

function syncSplitButton() {
  const n = chosenSplitRefs().length;
  const btn = document.getElementById('split-add');
  btn.disabled = !n;
  btn.textContent = n ? `Add ${n} side${n === 1 ? '' : 's'}` : 'Add sides';
}

function addSplitSides() {
  const refs = chosenSplitRefs();
  if (!refs.length) return;
  const s = currentState();
  const asBase = document.getElementById('split-as-base').checked;
  const base = asBase ? refs[0] : s.base;
  const add = asBase ? refs.slice(1) : refs;
  const other = [...s.other.filter((o) => !add.includes(o)), ...add].filter((o) => o !== base);
  const added = add.filter((r) => !s.other.includes(r) && r !== base).length;
  document.getElementById('split-msg').textContent = asBase
    ? `Baseline ${sideInfo(base).label}` + (added ? `, ${added} side${added === 1 ? '' : 's'} added.` : '.')
    : added ? `${added} side${added === 1 ? '' : 's'} added.` : 'Already picked.';
  split.values.clear();
  fillSplit();
  setState({ base, other });
}

/** Compared sides the lists above do not show (tag splits, sides from a link): ticked, untick to drop. */
function syncSplitCurrent(s) {
  const listed = new Set(allRefs());
  const extra = s.other.filter((o) => !listed.has(o));
  const box = clear(document.getElementById('split-current'));
  document.getElementById('split-picked').textContent = extra.length ? `(${extra.length} picked)` : '';
  if (!extra.length) { shownExtra = 0; return; }
  const checks = h('div', { class: 'checks' });
  for (const ref of extra) {
    const cb = h('input', { type: 'checkbox', value: ref, checked: true });
    cb.addEventListener('change', () => { if (!cb.checked) setState({ other: currentState().other.filter((o) => o !== ref) }); });
    checks.append(h('label', {}, cb, ...sideLabel(sideInfo(ref))));
  }
  box.append(h('fieldset', { class: 'picks' }, h('legend', { text: 'Picked sides not listed above' }), checks));
  if (extra.length > shownExtra) document.getElementById('pick-split').open = true;
  shownExtra = extra.length;
}
let shownExtra = 0;
let currentState = () => ({ base: '', other: [] });

function syncPicker(s) {
  if (pickerBuiltFor !== store.runs) buildPicker();
  const base = document.getElementById('base');
  // a reference that is not in the list (group:NAME@KEY=VALUE from a quick pick, id prefix, folder, label from
  // a link): keep it selectable, named like the listed sides when the data knows it
  base.querySelectorAll('option.from-link').forEach((o) => o.remove());
  if (s.base && ![...base.options].some((o) => o.value === s.base)) {
    const info = sideInfo(s.base);
    const text = info.known ? info.label + ' (' + info.sub + ')' : s.base + ' (from the link)';
    base.prepend(h('option', { class: 'from-link', value: s.base, text }));
  }
  base.value = s.base;
  for (const cb of document.querySelectorAll('#pick-groups input, #pick-runs input')) {
    cb.checked = s.other.includes(cb.value);
    cb.disabled = cb.value === s.base;
    cb.parentElement.title = cb.disabled ? 'This is the baseline' : '';
  }
  const runIds = new Set(store.runById.keys());
  const nRuns = s.other.filter((o) => runIds.has(o)).length;
  document.getElementById('runs-picked').textContent = nRuns ? `(${nRuns} picked)` : '';
  if (nRuns) document.querySelector('#view-compare details.picks-runs').open = true;
  syncSplitCurrent(s);
  const key = JSON.stringify(s.other);
  for (const b of document.querySelectorAll('#presets button')) {
    b.setAttribute('aria-pressed', String(b.dataset.base === s.base && b.dataset.others === key));
  }
}

// ---- render ----
let renderSeq = 0;

export function renderCompare(s) {
  currentState = () => s;
  syncPicker(s);
  syncExport(s);
  const seq = ++renderSeq;
  const status = compareStatus();
  const body = document.getElementById('compare-body');
  const takeaway = document.getElementById('takeaway');
  const warnings = document.getElementById('warnings');
  if (!s.base || !s.other.length) {
    clear(takeaway);
    clear(warnings);
    body.hidden = true;
    clear(status, stateBlock('empty', s.base ? 'Pick at least one group or run to compare with the baseline.' : 'Pick a baseline.'));
    return;
  }
  clear(status, stateBlock('loading', 'Comparing…'));
  getComparison(s.base, s.other).then((c) => {
    if (seq !== renderSeq) return;
    clear(status);
    body.hidden = false;
    renderTakeaway(c);
    clear(warnings, ...c.warnings.map((w) => h('li', { text: w })));
    renderTally(c);
    renderSectionSelect(c, s);
    renderHeat(c, s);
  }).catch((err) => {
    if (seq !== renderSeq) return;
    body.hidden = true;
    clear(takeaway);
    clear(warnings);
    clear(status, errorBlock(err, 'Cannot compare'));
  });
}

// ---- export: the static HTML report of the current selection (only where a server can build it) ----
function exportFileName(s) {
  const part = (ref) => ref.replace(/^group:/, '').replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40) || 'side';
  return 'benchlens-' + [s.base, ...s.other.slice(0, 3)].map(part).join('-vs-') + '.html';
}

function syncExport(s) {
  const btn = document.getElementById('export-html');
  const status = document.getElementById('export-status');
  btn.hidden = typeof api.exportHtml !== 'function';
  if (btn.hidden) return;
  btn.disabled = !s.base || !s.other.length;
  btn.onclick = async () => {
    btn.disabled = true;
    status.textContent = 'Building the report…';
    try {
      const html = await api.exportHtml(s.base, s.other);
      const url = URL.createObjectURL(new Blob([html], { type: 'text/html' }));
      const a = h('a', { href: url, download: exportFileName(s) });
      document.body.append(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 10000);
      status.textContent = '';
    } catch (err) {
      status.textContent = 'Cannot export: ' + (err && err.message ? err.message : String(err));
    } finally {
      btn.disabled = false;
    }
  };
}

function compareStatus() {
  let el = document.getElementById('compare-status');
  if (!el) {
    el = h('div', { id: 'compare-status', 'aria-live': 'polite' });
    document.getElementById('compare-body').before(el);
  }
  return el;
}

function renderTakeaway(c) {
  const t = c.tallies[0], o = c.others[0];
  const el = document.getElementById('takeaway');
  if (!t || !o) { clear(el); return; }
  const cross = c.rows.filter((r) => r.other === 0 && r.cross_arch).length;
  clear(el, 'Against ', h('b', { text: c.base.label }), ', ', h('b', { text: o.label }), ' is better in ',
    h('span', { class: 'num good', text: String(t.better) }), ' results, worse in ',
    h('span', { class: 'num bad', text: String(t.worse) }), ', and ',
    h('span', { class: 'num', text: String(t.not_clear) }), ' are within noise.',
    cross ? ` ${cross} sysbench result${cross > 1 ? 's are' : ' is'} shown but not counted (different architectures).` : '',
    c.others.length > 1 ? ` The table scores the other ${c.others.length - 1} side${c.others.length > 2 ? 's' : ''} too.` : '');
}

function renderTally(c) {
  const sections = sortSections([...new Set(c.tallies.flatMap((t) => Object.keys(t.by_section)))]);
  const table = document.getElementById('tally');
  const head = h('tr', {}, h('th', { scope: 'col', text: 'Compared with ' + c.base.label }),
    h('th', { scope: 'col', text: 'Better' }), h('th', { scope: 'col', text: 'Worse' }), h('th', { scope: 'col', text: 'Within noise' }),
    ...sections.map((s) => h('th', { scope: 'col', title: 'better / worse / within noise', text: sectionName(s) })));
  const rows = c.tallies.map((t, i) => {
    const side = c.others[i];
    return h('tr', {},
      h('th', { scope: 'row', class: 'left', text: side.label }),
      h('td', { class: 'good', text: String(t.better) }),
      h('td', { class: 'bad', text: String(t.worse) }),
      h('td', { text: String(t.not_clear) }),
      ...sections.map((s) => {
        const v = t.by_section[s];
        if (!v) return h('td', { class: 'sec', text: '–' });
        return h('td', { class: 'sec', 'aria-label': `${v[0]} better, ${v[1]} worse, ${v[2]} within noise` },
          h('span', { class: 'good', text: String(v[0]) }), h('span', { class: 'bad', text: String(v[1]) }), h('span', { text: String(v[2]) }));
      }));
  });
  clear(table, h('thead', {}, head), h('tbody', {}, rows));
}

function sectionChoices(c) {
  const present = sortSections([...new Set(c.rows.map((r) => r.section))]);
  const ids = new Set(c.rows.map((r) => r.id));
  const hasKey = KEY_METRICS.some((id) => ids.has(id));
  return [...(hasKey ? [['key', 'Key metrics']] : []), ['all', 'All metrics'], ...present.map((s) => [s, sectionName(s)])];
}

function effectiveSection(c, s) {
  const ch = sectionChoices(c).map((x) => x[0]);
  return ch.includes(s.section) ? s.section : ch[0];
}

function renderSectionSelect(c, s) {
  const sel = document.getElementById('section');
  clear(sel, ...sectionChoices(c).map(([v, t]) => h('option', { value: v, text: t })));
  sel.value = effectiveSection(c, s);
  sel.onchange = () => setState({ section: sel.value }, { replace: true });
}

/** Metric ids of the heatmap, in order, with section breaks for "all". */
export function metricIds(c, section) {
  const order = [];
  const seen = new Set();
  for (const r of c.rows) if (!seen.has(r.id)) { seen.add(r.id); order.push(r); }
  if (section === 'key') return KEY_METRICS.filter((id) => seen.has(id));
  if (section === 'all') {
    const secs = sortSections([...new Set(order.map((r) => r.section))]);
    return secs.flatMap((s) => order.filter((r) => r.section === s).map((r) => r.id));
  }
  return order.filter((r) => r.section === section).map((r) => r.id);
}

function cellColour(r) {
  const strength = 25 + 55 * Math.min(1, Math.abs(r.diff || 0) / 50);
  return `color-mix(in srgb, var(${r.verdict === 'better' ? '--good' : '--bad'}) ${strength.toFixed(0)}%, var(--bg))`;
}

function verdictLine(r) {
  if (r.cross_arch) return 'Not counted: sysbench across architectures';
  return (r.verdict === 'not_clear' ? 'Within noise' : r.verdict === 'better' ? 'Better than the baseline' : 'Worse than the baseline')
    + ` (${pct(r.diff)}, noise ${pct1(r.noise)})`;
}

export function tipLines(c, r) {
  const side = c.others[r.other];
  return [
    `${r.name} · ${side.label}`,
    `${c.base.label} ${fmt(r.base)} → ${side.label} ${fmt(r.value)} ${r.unit}`,
    verdictLine(r),
    `CV ${pct1(r.cv_base)} / ${pct1(r.cv_value)} · values ${r.n_base} / ${r.n_value} · ${r.direction === 'lower' ? 'lower' : 'higher'} is better`,
  ];
}

function renderHeat(c, s) {
  const section = effectiveSection(c, s);
  const ids = metricIds(c, section);
  const cell = new Map(c.rows.map((r) => [r.other + '|' + r.id, r]));
  const first = new Map();
  for (const r of c.rows) if (!first.has(r.id)) first.set(r.id, r);
  const table = document.getElementById('heat');
  const head = h('tr', {}, h('th', { scope: 'col', class: 'sr-only', text: 'Metric' }),
    ...c.others.map((o) => h('th', { scope: 'col' }, o.label,
      h('span', { class: 'sub', text: [o.archs.map(archName).join('+'), o.run_ids.length + (o.run_ids.length === 1 ? ' run' : ' runs')].filter(Boolean).join(' · ') }))));
  const body = h('tbody');
  let lastSec = null;
  for (const id of ids) {
    const m = first.get(id);
    if (section === 'all' && m.section !== lastSec) {
      lastSec = m.section;
      body.append(h('tr', {}, h('th', { scope: 'rowgroup', class: 'sec-head', colspan: String(c.others.length + 1), text: sectionName(m.section) })));
    }
    const open = () => setState({ view: 'metric', metric: id });
    const tr = h('tr', {}, h('th', { scope: 'row' },
      h('button', { type: 'button', text: m.name, title: `${m.name} (${m.unit}) · show every value`, onclick: open })));
    c.others.forEach((o, k) => {
      const r = cell.get(k + '|' + id);
      if (!r) { tr.append(h('td', {}, h('span', { class: 'cell missing', text: 'no data' }))); return; }
      const once = Math.min(r.n_base, r.n_value) < 2;
      const cls = r.cross_arch ? 'cross' : r.verdict;
      const span = h('span', { class: 'cell ' + cls }, pct(r.diff) + (once ? '·' : ''),
        h('span', { class: 'sr-only', text: ' ' + (r.cross_arch ? 'not counted' : VERDICT_TEXT[r.verdict]) }));
      if (!r.cross_arch && r.verdict !== 'not_clear') span.style.background = cellColour(r);
      span.addEventListener('pointerenter', (ev) => showTip(ev, tipLines(c, r)));
      span.addEventListener('pointermove', (ev) => showTip(ev, tipLines(c, r)));
      span.addEventListener('pointerleave', hideTip);
      span.addEventListener('click', open);
      tr.append(h('td', {}, span));
    });
    body.append(tr);
  }
  if (!ids.length) body.append(h('tr', {}, h('td', { class: 'muted', text: 'No metric of this section was compared.' })));
  clear(table, h('caption', { class: 'sr-only', text: `Difference from ${c.base.label} per metric` }), h('thead', {}, head), body);
}
