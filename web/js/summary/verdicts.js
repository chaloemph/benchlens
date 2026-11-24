// Summary block: Verdicts by section. One diverging bar per section for each compared side, drawn from
// `comparison.tallies[].by_section` as the API returns it ([better, worse, not_clear] per section); nothing is
// recomputed or re-judged here, so the counts agree with `benchlens compare`. Worse goes to the left, better to
// the right, within noise is a grey segment in the centre. Colour is never the only cue: the counts are written
// at the bar ends, worse bars carry a hatch, and the table fallback has the same numbers.

import { h, clear, showTip, hideTip } from '../ui.js';
import { sortSections, sectionName } from '../format.js';
import { tableFallback } from './table.js';

const SVGNS = 'http://www.w3.org/2000/svg';
const LABEL_W = 92;   // section name column
const COUNT_W = 30;   // room for the count written beyond a bar end
const ROW_H = 24;
const BAR_H = 14;
const MIN_W = 280;
const MAX_W = 760;

let chartSeq = 0;

/**
 * Rows of one tally for the chart: the sections of `by_section` in the UI's order, then a final "All sections"
 * row taken from the tally's own totals. `null` entries are skipped.
 */
export function verdictRows(tally) {
  const by = (tally && tally.by_section) || {};
  const rows = sortSections(Object.keys(by)).map((sec) => {
    const [better, worse, notClear] = by[sec];
    return { key: sec, label: sectionName(sec), better, worse, notClear, total: better + worse + notClear };
  });
  rows.push({ key: 'all', label: 'All sections', better: tally.better, worse: tally.worse, notClear: tally.not_clear,
    total: tally.better + tally.worse + tally.not_clear, isTotal: true });
  return rows;
}

const countsText = (r) => `better ${r.better} · worse ${r.worse} · within noise ${r.notClear}`;

function svgEl(parent, tag, attrs) {
  const el = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs || {})) el.setAttribute(k, String(v));
  parent.appendChild(el);
  return el;
}

/** One side: heading, the SVG and nothing else. `half` is the shared half-width in counts (same scale for every side). */
function sideChart(side, tally, rows, half, width) {
  const d3 = window.d3;
  const id = 'sum-verdicts-hatch-' + (++chartSeq);
  const barX0 = LABEL_W + COUNT_W;
  const barX1 = width - COUNT_W;
  const x = d3.scaleLinear().domain([-half, half]).range([barX0, barX1]);
  const mid = x(0);
  const height = rows.length * ROW_H + 8;

  const svg = svgEl(document.createElement('div'), 'svg', {
    viewBox: `0 0 ${width} ${height}`, width, height, role: 'img',
    'aria-label': `Verdicts by section for ${side.label}: ${countsText(rows[rows.length - 1])}`,
  });
  const defs = svgEl(svg, 'defs');
  const pat = svgEl(defs, 'pattern', { id, width: 6, height: 6, patternUnits: 'userSpaceOnUse', patternTransform: 'rotate(45)' });
  svgEl(pat, 'line', { x1: 0, y1: 0, x2: 0, y2: 6, class: 'sum-verdicts-hatch', 'stroke-width': 2 });

  rows.forEach((r, i) => {
    const y = 4 + i * ROW_H;
    const cy = y + ROW_H / 2;
    const g = svgEl(svg, 'g', { class: 'sum-verdicts-row' + (r.isTotal ? ' total' : ''), tabindex: 0 });
    const lines = [side.label + ' · ' + r.label, countsText(r), r.total + (r.total === 1 ? ' result' : ' results')];
    g.addEventListener('pointerenter', (ev) => showTip(ev, lines));
    g.addEventListener('pointermove', (ev) => showTip(ev, lines));
    g.addEventListener('pointerleave', hideTip);
    g.addEventListener('focus', (ev) => showTip(ev, lines));
    g.addEventListener('blur', hideTip);
    svgEl(g, 'rect', { x: 0, y, width, height: ROW_H, class: 'sum-verdicts-hit' });
    svgEl(g, 'text', { x: 0, y: cy, dy: '0.34em', class: 'lbl' + (r.isTotal ? ' base' : '') }).textContent = r.label + ' (' + r.total + ')';
    if (r.total === 0) {
      svgEl(g, 'text', { x: mid, y: cy, dy: '0.34em', 'text-anchor': 'middle' }).textContent = 'no results';
      return;
    }
    const nHalf = r.notClear / 2;
    const noiseL = x(-nHalf), noiseR = x(nHalf);
    const top = cy - BAR_H / 2;
    // worse: from the left edge of the noise segment to the left; better: from its right edge to the right
    if (r.worse) {
      const wx = x(-nHalf - r.worse);
      svgEl(g, 'rect', { x: wx, y: top, width: noiseL - wx, height: BAR_H, class: 'sum-verdicts-worse' });
      svgEl(g, 'rect', { x: wx, y: top, width: noiseL - wx, height: BAR_H, fill: `url(#${id})` });
    }
    if (r.notClear) svgEl(g, 'rect', { x: noiseL, y: top, width: noiseR - noiseL, height: BAR_H, class: 'sum-verdicts-noise' });
    if (r.better) svgEl(g, 'rect', { x: noiseR, y: top, width: x(nHalf + r.better) - noiseR, height: BAR_H, class: 'sum-verdicts-better' });
    // counts: worse at the left end, better at the right end, noise in its segment when it is wide enough
    svgEl(g, 'text', { x: x(-nHalf - r.worse) - 4, y: cy, dy: '0.34em', 'text-anchor': 'end', class: 'lbl' }).textContent = r.worse;
    svgEl(g, 'text', { x: x(nHalf + r.better) + 4, y: cy, dy: '0.34em', class: 'lbl' }).textContent = r.better;
    if (noiseR - noiseL >= 16) svgEl(g, 'text', { x: mid, y: cy, dy: '0.34em', 'text-anchor': 'middle', class: 'sum-verdicts-nlabel' }).textContent = r.notClear;
  });
  // the centre line: the same place for every row
  svgEl(svg, 'line', { x1: mid, x2: mid, y1: 2, y2: height - 2, class: 'sum-verdicts-axis' });
  return svg.parentNode;
}

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  const cmp = ctx.comparison;
  const sides = cmp.others || [];
  const tallies = cmp.tallies || [];
  const per = sides.map((side, i) => ({ side, rows: tallies[i] ? verdictRows(tallies[i]) : [] }));
  const width = Math.max(MIN_W, Math.min(MAX_W, Math.floor(el.clientWidth || 640)));

  // one scale for every side and section: half-width = the largest (bigger of worse / better + half the noise)
  let half = 1;
  for (const p of per) for (const r of p.rows) half = Math.max(half, Math.max(r.better, r.worse) + r.notClear / 2);

  const legend = h('div', { class: 'legend' },
    h('span', {}, h('i', { class: 'sw sum-verdicts-sw-worse' }), ' worse'),
    h('span', {}, h('i', { class: 'sw sum-verdicts-sw-noise' }), ' within noise'),
    h('span', {}, h('i', { class: 'sw sum-verdicts-sw-better' }), ' better'));

  const parts = [
    h('h2', { text: 'Verdicts by section' }),
    h('p', { class: 'muted', text: 'Results counted as better, worse or within noise against the baseline '
      + (cmp.base ? cmp.base.label : '') + ', as vmbench judges them. Bars share one scale.' }),
    legend,
  ];
  for (const p of per) {
    const box = h('div', { class: 'chart sum-verdicts-side' });
    if (p.rows.length) box.append(sideChart(p.side, tallies[per.indexOf(p)], p.rows, half, width));
    parts.push(h('section', { class: 'sum-verdicts-block' },
      h('h3', { text: p.side.label }),
      p.rows.length ? h('p', { class: 'muted', text: countsText(p.rows[p.rows.length - 1]) }) : null,
      p.rows.length ? box : h('p', { class: 'muted', text: 'No tally for this side.' })));
  }

  const cross = (cmp.rows || []).some((r) => r.cross_arch);
  if (cross) parts.push(h('p', { class: 'muted sum-verdicts-note', text: 'sysbench is not counted across architectures: those results are shown in Compare but left out of these bars.' }));

  // table fallback: one row per side and section, the same numbers
  const rows = [];
  for (const p of per) for (const r of p.rows) rows.push([p.side.label, r.label, r.better, r.worse, r.notClear, r.total]);
  parts.push(tableFallback({ label: 'Verdicts by section', columns: ['Side', 'Section', 'Better', 'Worse', 'Within noise', 'Results'], rows }));
  clear(el, ...parts);
}
