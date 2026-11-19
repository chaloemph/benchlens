// Metric detail: a dot plot of every raw value per side (GET /api/values), coloured by the run's `round` tag
// when every run has one, else by run. The median tick and the statistics table come from GET /api/compare.

import { api } from './api.js';
import { store } from './data.js';
import { setState } from './state.js';
import { getComparison, metricIds, KEY_METRICS } from './compare.js';
import { h, clear, stateBlock, errorBlock, showTip, hideTip } from './ui.js';
import { fmt, pct, pct1, sectionName, sortSections, VERDICT_TEXT } from './format.js';

const PALETTE = ['#4e79a7', '#f28e2b', '#59a14f', '#e15759', '#9c6fb6', '#76b7b2', '#d4a72c', '#ff9da7', '#9c755f', '#8f8f8f'];

const valueCache = new Map();
function getValues(ref, id) {
  const k = ref + '\n' + id;
  if (!valueCache.has(k)) {
    const p = api.values(ref, id);
    valueCache.set(k, p);
    p.catch(() => valueCache.delete(k));
  }
  return valueCache.get(k);
}
export function dropValueCache() { valueCache.clear(); }

let seq = 0;

export function renderMetric(s) {
  const my = ++seq;
  const chart = document.getElementById('dots');
  const statsTable = document.getElementById('metric-stats');
  const desc = document.getElementById('metric-desc');
  const legend = document.getElementById('dot-legend');
  document.getElementById('back-compare').onclick = () => setState({ view: 'compare' });
  if (!s.base || !s.other.length) {
    clear(chart, stateBlock('empty', 'Pick a comparison first (Compare view); this page shows every value of one of its metrics.'));
    clear(statsTable); clear(desc); clear(legend); clear(document.getElementById('metric'));
    return;
  }
  clear(chart, stateBlock('loading', 'Loading values…'));
  getComparison(s.base, s.other).then((c) => {
    if (my !== seq) return null;
    const sel = document.getElementById('metric');
    const ids = metricIds(c, 'all');
    if (!ids.length) { clear(chart, stateBlock('empty', 'Nothing was compared.')); return null; }
    const firstRow = new Map();
    for (const r of c.rows) if (!firstRow.has(r.id)) firstRow.set(r.id, r);
    const groups = sortSections([...new Set(ids.map((id) => firstRow.get(id).section))]);
    clear(sel, ...groups.map((g) => h('optgroup', { label: sectionName(g) },
      ...ids.filter((id) => firstRow.get(id).section === g).map((id) => h('option', { value: id, text: `${firstRow.get(id).name} (${firstRow.get(id).unit})` })))));
    const metric = ids.includes(s.metric) ? s.metric : (KEY_METRICS.find((id) => ids.includes(id)) || ids[0]);
    sel.value = metric;
    sel.onchange = () => setState({ metric: sel.value }, { replace: true });
    const m = firstRow.get(metric);
    const sides = [c.base, ...c.others];
    return Promise.all(sides.map((side) => getValues(side.reference, metric))).then((vals) => {
      if (my !== seq) return;
      const tool = vals.flatMap((v) => v.values).map((v) => v.tool).find(Boolean);
      desc.textContent = `${m.name} · ${m.unit} · ${m.direction === 'lower' ? 'lower' : 'higher'} is better · ${sectionName(m.section)}${tool ? ' · ' + tool : ''} · id ${m.id}`;
      draw(chart, legend, c, metric, sides, vals.map((v) => v.values));
      renderStats(statsTable, c, metric);
    });
  }).catch((err) => {
    if (my !== seq) return;
    clear(chart, errorBlock(err, 'Cannot load this metric'));
    clear(statsTable);
  });
}

function medianOf(c, metric, sideIndex) {
  const rows = c.rows.filter((r) => r.id === metric);
  if (sideIndex === 0) return rows.length ? rows[0].base : null;
  const r = rows.find((x) => x.other === sideIndex - 1);
  return r ? r.value : null;
}

function draw(el, legend, c, metric, sides, values) {
  el.replaceChildren();
  const d3 = window.d3;
  const rows = sides.map((side, i) => ({ side, i, pts: values[i], median: medianOf(c, metric, i) }));
  const all = rows.flatMap((r) => r.pts.map((p) => p.value));
  if (!all.length) { el.append(stateBlock('empty', 'This metric has no values on these sides.')); clear(legend); return; }

  // colour by the run's round tag when every run has one, else by run
  const runIds = [...new Set(rows.flatMap((r) => r.pts.map((p) => p.run_id)))];
  const runOf = (id) => store.runById.get(id);
  const byRound = runIds.every((id) => runOf(id) && runOf(id).tags.round);
  const keyOf = (p) => (byRound ? runOf(p.run_id).tags.round : p.run_id);
  const keys = byRound ? [...new Set(runIds.map((id) => runOf(id).tags.round))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })) : runIds;
  const colour = (k) => PALETTE[keys.indexOf(k) % PALETTE.length];
  const keyLabel = (k) => (byRound ? 'round ' + k : (runOf(k) ? runOf(k).label : k));
  clear(legend, h('span', { class: 'muted', text: byRound ? 'Colour: test round (run tag round)' : 'Colour: run' }),
    ...keys.map((k) => { const sw = h('i', { class: 'dot-sw' }); sw.style.background = colour(k); return h('span', {}, sw, keyLabel(k)); }));

  const W = Math.max(300, el.clientWidth || 600);
  const rowH = 40;
  const svg = d3.select(el).append('svg').attr('role', 'img');
  const probe = svg.append('text').attr('class', 'lbl base');
  let lw = 0;
  for (const r of rows) { probe.text(r.side.label); lw = Math.max(lw, probe.node().getComputedTextLength()); }
  probe.text('8,888,888'); const vw = probe.node().getComputedTextLength(); probe.remove();
  const labelW = Math.min(Math.ceil(lw) + 12, Math.floor(W * 0.38));
  const mg = { l: labelW, r: Math.ceil(vw) + 12, t: 8, b: 30 };
  const H = mg.t + rows.length * rowH + mg.b;
  svg.attr('viewBox', `0 0 ${W} ${H}`).attr('width', W).attr('height', H);
  const lo = d3.min(all), hi = d3.max(all), pad = (hi - lo || Math.abs(hi) || 1) * 0.08;
  const x = d3.scaleLinear().domain([lo - pad, hi + pad]).range([mg.l, W - mg.r]).nice();
  const ticks = Math.max(2, Math.floor((W - mg.l - mg.r) / 80));
  svg.append('g').attr('class', 'grid').selectAll('line').data(x.ticks(ticks)).join('line')
    .attr('x1', x).attr('x2', x).attr('y1', mg.t).attr('y2', H - mg.b);
  svg.append('g').attr('class', 'axis').attr('transform', `translate(0,${H - mg.b})`)
    .call(d3.axisBottom(x).ticks(ticks).tickFormat(fmt)).call((g) => g.select('.domain').remove());

  const unit = rows.flatMap((r) => r.pts)[0].unit;
  rows.forEach((r) => {
    const y = mg.t + r.i * rowH + rowH / 2;
    const t = svg.append('text').attr('class', 'lbl' + (r.i === 0 ? ' base' : '')).attr('x', 0).attr('y', y + 4).text(r.side.label);
    let txt = r.side.label;
    while (txt.length > 4 && t.node().getComputedTextLength() > labelW - 8) { txt = txt.slice(0, -2); t.text(txt + '…'); }
    t.append('title').text(r.side.label + (r.i === 0 ? ' (baseline)' : ''));
    if (!r.pts.length) {
      svg.append('text').attr('x', mg.l).attr('y', y + 4).text('not measured');
      return;
    }
    // one lane per run inside the row, so the runs of a pooled group separate a little
    const lanes = [...new Set(r.pts.map((p) => p.run_id))];
    const laneY = (p, k) => {
      const n = lanes.length, li = lanes.indexOf(p.run_id);
      const off = n > 1 ? (li - (n - 1) / 2) * Math.min(7, 22 / (n - 1)) : 0;
      return y + off + ((k % 3) - 1) * 1.2;
    };
    if (r.median != null) {
      svg.append('line').attr('class', 'median').attr('x1', x(r.median)).attr('x2', x(r.median)).attr('y1', y - 14).attr('y2', y + 14);
      svg.append('text').attr('class', 'lbl num').attr('x', W - mg.r + 8).attr('y', y + 4).text(fmt(r.median));
    }
    svg.append('g').selectAll('circle').data(r.pts).join('circle')
      .attr('cx', (p) => x(p.value)).attr('cy', laneY).attr('r', 4.5)
      .style('fill', (p) => colour(keyOf(p))).style('fill-opacity', 0.85)
      .on('pointerenter pointermove', (ev, p) => {
        const run = runOf(p.run_id);
        showTip(ev, [
          `${r.side.label} · ${run ? run.label : p.run_id}`,
          `${fmt(p.value)} ${p.unit}`,
          [run && run.tags.round ? 'test round ' + run.tags.round : '', 'iteration ' + p.round, run ? 'vmbench ' + run.vmbench_version : ''].filter(Boolean).join(' · '),
          'run ' + p.run_id,
        ]);
      })
      .on('pointerleave', hideTip);
  });
  const med = rows.map((r) => `${r.side.label} ${r.median == null ? 'no median' : fmt(r.median)}`).join(', ');
  svg.attr('aria-label', `Dot plot of ${all.length} values of ${rows[0].pts[0] ? rows[0].pts[0].name : metric} (${unit}) on ${rows.length} sides; axis not from zero. Medians: ${med}.`);
}

function renderStats(table, c, metric) {
  const rows = c.rows.filter((r) => r.id === metric);
  const head = h('tr', {}, ...['Side', 'Median', 'CV', 'Values', 'Difference', 'Noise', 'Verdict'].map((t) => h('th', { scope: 'col', text: t })));
  const body = h('tbody');
  if (rows.length) {
    const r0 = rows[0];
    body.append(h('tr', {}, h('th', { scope: 'row', class: 'left', text: c.base.label + ' (baseline)' }),
      h('td', { text: fmt(r0.base) + ' ' + r0.unit }), h('td', { text: pct1(r0.cv_base) }), h('td', { text: String(r0.n_base) }),
      h('td', { text: '—' }), h('td', { text: '—' }), h('td', { text: '—' })));
  }
  for (const r of rows) {
    const verdict = r.cross_arch ? 'not counted (sysbench across architectures)' : VERDICT_TEXT[r.verdict];
    body.append(h('tr', {}, h('th', { scope: 'row', class: 'left', text: c.others[r.other].label }),
      h('td', { text: fmt(r.value) + ' ' + r.unit }), h('td', { text: pct1(r.cv_value) }), h('td', { text: String(r.n_value) }),
      h('td', { text: pct(r.diff) }), h('td', { text: pct1(r.noise) }),
      h('td', { class: r.cross_arch ? '' : r.verdict === 'better' ? 'good' : r.verdict === 'worse' ? 'bad' : '', text: verdict })));
  }
  clear(table, h('thead', {}, head), body);
}
