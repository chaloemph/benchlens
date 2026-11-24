// Summary block: Radar. One axis per section in the fixed order cpu, disk, mem, net, pg, web (never reordered by
// data); the baseline is the dashed ring at 100, up to 4 compared sides are polygons with their own dash and
// marker shape. All scores come from ctx.scores (score.js); this file only places them on the chart.

import { h, clear, showTip, hideTip } from '../ui.js';
import { SECTION_NAMES } from '../format.js';
import { SECTIONS } from '../score.js';
import { tableFallback } from './table.js';
import { blockTitle, fmtScore, countsText } from './common.js';

export const MAX_SIDES = 4;

// stroke dash and marker shape per drawn side, so colour is never the only cue (the baseline is dashed too, ring only)
const DASHES = ['', '8 3', '2 3', '10 3 2 3'];
const SHAPES = ['circle', 'square', 'triangle', 'diamond'];

const secName = (s) => SECTION_NAMES[s] || s;

/** Radial scale top: 160 or the next multiple of 20 above the largest drawn score. */
export function radarMax(scoresList) {
  const top = Math.max(0, ...scoresList.filter((v) => typeof v === 'number' && Number.isFinite(v)));
  return Math.max(160, Math.ceil(top / 20) * 20);
}

/** Ring step: 20, or 40 once the scale is taller than 160. */
export const ringStep = (max) => (max > 160 ? 40 : 20);

/** Point on axis `i` of `n` (first axis straight up, clockwise) at radius `r`, around (0, 0). */
export function axisPoint(i, n, r) {
  const a = (2 * Math.PI * i) / n - Math.PI / 2;
  return [r * Math.cos(a), r * Math.sin(a)];
}

/**
 * Runs of the outline of a side: `[ [i, i+1, ...], ... ]` lists of consecutive axes (cyclic) that have a score,
 * so a missing axis is a gap in the line and never a dip to 0. All axes present: one closed run.
 */
export function outlineRuns(present) {
  const n = present.length;
  if (present.every(Boolean)) return { closed: true, runs: [present.map((_, i) => i)] };
  const runs = [];
  const start = present.findIndex((p) => !p);
  let cur = [];
  for (let k = 1; k <= n; k++) {
    const i = (start + k) % n;
    if (present[i]) cur.push(i);
    else if (cur.length) { runs.push(cur); cur = []; }
  }
  if (cur.length) runs.push(cur);
  return { closed: false, runs };
}

function symbolType(shape) {
  return { circle: d3.symbolCircle, square: d3.symbolSquare, triangle: d3.symbolTriangle, diamond: d3.symbolDiamond }[shape];
}

function tooltipLines(side, sec, cell) {
  return [
    side.label,
    secName(sec) + ': score (summary) ' + (cell.score == null ? 'no data' : fmtScore(cell.score)),
    cell.n + ' results: ' + countsText(cell.counts) + (cell.weak && cell.n ? ' · few results' : ''),
  ];
}

function draw(el, ctx, sides) {
  const W = 420, H = 400, cx = W / 2, cy = H / 2 + 4, R = 120;
  const n = SECTIONS.length;
  const max = radarMax(sides.flatMap((s) => SECTIONS.map((sec) => s.sections[sec].score)));
  const rad = (v) => (v / max) * R;
  const svg = d3.select(el).append('svg').attr('class', 'sum-radar-svg').attr('viewBox', `0 0 ${W} ${H}`)
    .attr('role', 'img').attr('aria-label', 'Radar of section scores (summary), baseline = 100');
  const g = svg.append('g').attr('transform', `translate(${cx},${cy})`);
  const ring = (v) => d3.range(n).map((i) => axisPoint(i, n, rad(v)).join(',')).join(' ');

  // rings and spokes
  const step = ringStep(max);
  for (let v = step; v <= max; v += step) {
    g.append('polygon').attr('class', 'sum-radar-ring').attr('points', ring(v));
    const [, ty] = axisPoint(0, n, rad(v));
    g.append('text').attr('class', 'sum-radar-tick').attr('x', 3).attr('y', ty + 11).text(String(v));
  }
  d3.range(n).forEach((i) => {
    const [x, y] = axisPoint(i, n, R);
    g.append('line').attr('class', 'sum-radar-spoke').attr('x2', x).attr('y2', y);
  });
  // baseline: dashed ring at 100
  g.append('polygon').attr('class', 'sum-radar-base').attr('points', ring(100))
    .append('title').text('Baseline = 100');

  // axis labels: name, n, "no data" / "few results"
  SECTIONS.forEach((sec, i) => {
    const cells = sides.map((s) => s.sections[sec]);
    const withData = cells.filter((c) => c.score != null);
    const ns = cells.map((c) => c.n);
    const lo = Math.min(...ns), hi = Math.max(...ns);
    const nText = lo === hi ? String(lo) : lo + '–' + hi;
    const noData = withData.length === 0;
    const weak = !noData && withData.every((c) => c.weak);
    const [x, y] = axisPoint(i, n, R + 16);
    const anchor = Math.abs(x) < 4 ? 'middle' : x > 0 ? 'start' : 'end';
    const t = g.append('text').attr('class', 'sum-radar-label' + (weak || noData ? ' faded' : '')).attr('text-anchor', anchor)
      .attr('x', x).attr('y', y + (Math.abs(x) < 4 ? (y < 0 ? -6 : 14) : 0));
    t.append('tspan').attr('x', x).text(secName(sec) + ' (' + nText + ')');
    const note = noData ? 'no data' : weak ? 'few results' : '';
    if (note) t.append('tspan').attr('x', x).attr('dy', '1.15em').text(note);
  });

  // sides
  sides.forEach((side, k) => {
    const cls = 'sum-radar-s' + k;
    const cells = SECTIONS.map((sec) => side.sections[sec]);
    const pt = (i) => axisPoint(i, n, rad(cells[i].score));
    const { closed, runs } = outlineRuns(cells.map((c) => c.score != null));
    const sg = g.append('g').attr('class', 'sum-radar-side ' + cls);
    const present = cells.map((c, i) => (c.score != null ? i : -1)).filter((i) => i >= 0);
    if (present.length >= 3) {
      sg.append('polygon').attr('class', 'sum-radar-fill').attr('points', present.map((i) => pt(i).join(',')).join(' '));
    }
    const dash = DASHES[k % DASHES.length];
    const edge = (a, b) => {
      const weak = cells[a].weak || cells[b].weak;
      const [x1, y1] = pt(a), [x2, y2] = pt(b);
      sg.append('line').attr('class', 'sum-radar-edge' + (weak ? ' faded' : '')).attr('stroke-dasharray', dash || null)
        .attr('x1', x1).attr('y1', y1).attr('x2', x2).attr('y2', y2);
    };
    for (const run of runs) {
      for (let j = 0; j + 1 < run.length; j++) edge(run[j], run[j + 1]);
      if (closed) edge(run[run.length - 1], run[0]);
    }
    SECTIONS.forEach((sec, i) => {
      const cell = cells[i];
      if (cell.score == null) return;
      const [x, y] = pt(i);
      const lines = tooltipLines(side, sec, cell);
      const m = sg.append('path').attr('class', 'sum-radar-marker' + (cell.weak ? ' faded' : ''))
        .attr('d', d3.symbol().type(symbolType(SHAPES[k % SHAPES.length])).size(cell.weak ? 40 : 64)())
        .attr('transform', `translate(${x},${y})`).attr('tabindex', 0).attr('role', 'img')
        .attr('aria-label', lines.join('. '))
        .attr('data-side', side.label).attr('data-section', sec).attr('data-score', String(cell.score));
      m.on('mousemove', (ev) => showTip(ev, lines)).on('mouseleave', hideTip)
        .on('focus', (ev) => showTip(ev, lines)).on('blur', hideTip);
    });
  });
}

/** Legend: the baseline (dashed ring) and one entry per drawn side with its dash and marker. */
function legend(sides) {
  const sw = (cls, dash, shape) => {
    const s = d3.create('svg').attr('class', 'sum-radar-sw ' + cls).attr('viewBox', '0 0 34 12').attr('width', 34).attr('height', 12).attr('aria-hidden', 'true');
    s.append('line').attr('x1', 1).attr('x2', 33).attr('y1', 6).attr('y2', 6).attr('stroke-dasharray', dash || null);
    if (shape) s.append('path').attr('class', 'sum-radar-marker').attr('d', d3.symbol().type(symbolType(shape)).size(40)()).attr('transform', 'translate(17,6)');
    return s.node();
  };
  return h('div', { class: 'sum-radar-legend' },
    h('span', {}, sw('sum-radar-swbase', '4 3', null), 'baseline = 100'),
    ...sides.map((s, k) => h('span', {}, sw('sum-radar-s' + k, DASHES[k % DASHES.length], SHAPES[k % SHAPES.length]), s.label)));
}

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  const all = ctx.scores.sides;
  let picked = all.slice(0, MAX_SIDES).map((s) => s.index);

  const hint = h('p', { class: 'muted', text: 'Score (summary) per section, 100 = the baseline ' + ctx.scores.base.label
    + '; a result above 100 is better than the baseline. Summaries, not verdicts: the counts of better, worse and within-noise results are in each point\'s tooltip and in the table.' });
  const chart = h('div', { class: 'chart sum-radar-chart' });
  const tableSlot = h('div', {});
  const selector = h('fieldset', { class: 'sum-radar-pick', hidden: all.length <= MAX_SIDES });

  const table = () => {
    const rows = [];
    for (const side of all) {
      for (const sec of SECTIONS) {
        const c = side.sections[sec];
        rows.push([side.label, secName(sec), c.score == null ? 'no data' : fmtScore(c.score), c.n, c.counts.better, c.counts.worse, c.counts.not_clear, c.weak && c.n ? 'few results' : '']);
      }
    }
    return tableFallback({ label: 'Section scores (summary) per side', columns: ['Side', 'Section', 'Score (summary)', 'Results', 'Better', 'Worse', 'Within noise', 'Note'], rows });
  };

  const redraw = () => {
    const sides = all.filter((s) => picked.includes(s.index));
    clear(chart);
    hideTip();
    if (!sides.length) { chart.append(h('p', { class: 'muted', text: 'Pick at least one side to draw.' })); return; }
    draw(chart, ctx, sides);
    chart.append(legend(sides));
  };

  if (all.length > MAX_SIDES) {
    const msg = h('span', { class: 'muted', role: 'status' });
    const sync = () => {
      const full = picked.length >= MAX_SIDES;
      for (const box of selector.querySelectorAll('input')) box.disabled = full && !box.checked;
      msg.textContent = 'Up to ' + MAX_SIDES + ' sides are drawn (' + picked.length + ' picked).';
    };
    selector.append(h('legend', { text: 'Sides drawn' }));
    for (const side of all) {
      const box = h('input', { type: 'checkbox', checked: picked.includes(side.index) });
      box.addEventListener('change', () => {
        picked = all.map((s) => s.index).filter((i) => (i === side.index ? box.checked : picked.includes(i)));
        sync();
        redraw();
      });
      selector.append(h('label', {}, box, ' ', side.label));
    }
    selector.append(msg);
    sync();
  }

  redraw();
  tableSlot.append(table());
  clear(el, blockTitle('Section scores'), hint, selector, chart, tableSlot);
}
