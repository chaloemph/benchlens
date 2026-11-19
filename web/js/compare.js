// Compare view: baseline + compared sides, quick picks, takeaway, tally table, heatmap.
// Every number comes from GET /api/compare as it is; nothing is recomputed here.

import { api } from './api.js';
import { store, sideInfo, sortedGroups, sortedRuns, groupRef, quickPicks } from './data.js';
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
        const order = allRefs();
        const cur = s.other.filter((o) => o !== ref);
        const next = cb.checked ? [...cur, ref] : cur;
        next.sort((a, b) => order.indexOf(a) - order.indexOf(b));
        setState({ other: next });
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
  pickerBuiltFor = store.runs;
}

const allRefs = () => [...sortedGroups().map(groupRef), ...sortedRuns().map((r) => r.run_id)];
let currentState = () => ({ base: '', other: [] });

function syncPicker(s) {
  if (pickerBuiltFor !== store.runs) buildPicker();
  const base = document.getElementById('base');
  // a reference from a link that is not in the list (id prefix, folder, label): keep it selectable
  base.querySelectorAll('option.from-link').forEach((o) => o.remove());
  if (s.base && ![...base.options].some((o) => o.value === s.base)) {
    base.prepend(h('option', { class: 'from-link', value: s.base, text: s.base + ' (from the link)' }));
  }
  base.value = s.base;
  for (const cb of document.querySelectorAll('#pick-groups input, #pick-runs input')) {
    cb.checked = s.other.includes(cb.value);
    cb.disabled = cb.value === s.base;
    cb.parentElement.title = cb.disabled ? 'This is the baseline' : '';
  }
  const nRuns = s.other.filter((o) => !o.startsWith('group:')).length;
  document.getElementById('runs-picked').textContent = nRuns ? `(${nRuns} picked)` : '';
  if (nRuns) document.querySelector('#view-compare details.picks-runs').open = true;
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
