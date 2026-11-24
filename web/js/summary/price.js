// Summary block: Price / performance. Scatter of price (x) against the overall score (y, summary) with the
// Pareto frontier, a bar list of the performance-per-price index, and the sides that cannot be priced.
// The maths (price of a side, score / price, index, frontier) is in ../score.js (sidePrice, pricePerformance,
// paretoFrontier);
// this file only lays the numbers out. Prices, units and labels are untrusted text: textContent only.

import { h, clear, showTip, hideTip } from '../ui.js';
import { sidePrice, pricePerformance, paretoFrontier, PROFILES } from '../score.js';
import { tableFallback } from './table.js';

const NOTE = 'Price / performance: add a `price` tag to at least two sides to see it';
const REASONS = {
  none: 'no price tag',
  invalid: 'invalid price (a positive number with a decimal point is expected)',
  inconsistent: 'inconsistent price inside the side',
  'different unit': 'different unit',
  'no score': 'no score',
};

const num = (v) => (v == null ? '' : v.toFixed(1));
const fmtPrice = (p) => String(+p.toPrecision(6));
const unitText = (u) => u || 'no unit';
const trim = (s, n) => (s.length > n ? s.slice(0, n - 1) + '…' : s);

/** Better / worse / within noise counts of a side, summed over its sections (verdicts as vmbench judged them). */
function totalCounts(sections) {
  const t = { better: 0, worse: 0, not_clear: 0 };
  for (const s of Object.values(sections || {})) for (const k of Object.keys(t)) t[k] += s.counts[k];
  return t;
}
const countsText = (c) => (c ? `better ${c.better} · worse ${c.worse} · within noise ${c.not_clear}` : 'baseline: 100 by definition');

/** Entries for pricePerformance: the baseline (score 100) and every compared side, with their prices. */
function entriesOf(ctx) {
  const { comparison, runs, scores } = ctx;
  const entries = [{ key: 'base', label: comparison.base.label, score: scores.base.overall.score, price: sidePrice(comparison.base, runs), isBase: true, counts: null }];
  comparison.others.forEach((side, i) => {
    const sc = scores.sides[i];
    entries.push({ key: 'o' + i, label: side.label, score: sc.overall.score, price: sidePrice(side, runs), isBase: false, counts: totalCounts(sc.sections) });
  });
  return entries;
}

function drawScatter(el, pp, frontier) {
  const d3 = window.d3;
  const W = Math.max(300, Math.min(640, el.clientWidth || 640));
  const H = W < 480 ? 300 : 340;
  const mg = { l: 48, r: 16, t: 14, b: 46 };
  const svg = d3.select(el).append('svg').attr('viewBox', `0 0 ${W} ${H}`).attr('role', 'img')
    .attr('aria-label', 'Overall score (summary) against price, one point per side; the points on the line are not beaten on both price and score');
  const x = d3.scaleLinear().domain([0, d3.max(pp.usable, (e) => e.price) * 1.12]).range([mg.l, W - mg.r]).nice();
  const y = d3.scaleLinear().domain([0, d3.max(pp.usable, (e) => e.score) * 1.12]).range([H - mg.b, mg.t]).nice();
  svg.append('g').attr('class', 'grid').selectAll('line').data(y.ticks(5)).join('line')
    .attr('x1', mg.l).attr('x2', W - mg.r).attr('y1', (t) => y(t)).attr('y2', (t) => y(t));
  svg.append('g').attr('class', 'axis').attr('transform', `translate(0,${H - mg.b})`).call(d3.axisBottom(x).ticks(W < 480 ? 4 : 6));
  svg.append('g').attr('class', 'axis').attr('transform', `translate(${mg.l},0)`).call(d3.axisLeft(y).ticks(5));
  svg.append('text').attr('x', (mg.l + W - mg.r) / 2).attr('y', H - 8).attr('text-anchor', 'middle').text(`Price (${unitText(pp.unit)})`);
  svg.append('text').attr('transform', `translate(12,${(mg.t + H - mg.b) / 2}) rotate(-90)`).attr('text-anchor', 'middle').text('Score (summary)');

  const front = pp.usable.filter((e) => frontier.has(e.key)).sort((a, b) => a.price - b.price);
  if (front.length > 1) {
    svg.append('path').attr('class', 'sum-price-frontier').attr('fill', 'none')
      .attr('d', d3.line().x((e) => x(e.price)).y((e) => y(e.score))(front));
  }
  const pts = svg.append('g').selectAll('g').data(pp.usable).join('g')
    .attr('class', (e) => 'sum-price-pt ' + (frontier.has(e.key) ? 'front' : 'dominated'))
    .attr('transform', (e) => `translate(${x(e.price)},${y(e.score)})`)
    .attr('tabindex', 0).attr('role', 'img')
    .attr('aria-label', (e) => `${e.label}: score ${num(e.score)}, price ${fmtPrice(e.price)} ${unitText(e.unit)}${frontier.has(e.key) ? ', on the frontier' : ', beaten by another side'}`);
  // frontier: filled circle; beaten: hollow square, muted; the baseline's label says so
  pts.filter((e) => frontier.has(e.key)).append('circle').attr('r', 6);
  pts.filter((e) => !frontier.has(e.key)).append('rect').attr('x', -5).attr('y', -5).attr('width', 10).attr('height', 10);
  // labels: nudge one down when it would sit on an earlier one
  const placed = [];
  const offsets = new Map();
  for (const e of [...pp.usable].sort((a, b) => x(a.price) - x(b.price) || y(a.score) - y(b.score))) {
    let dy = -9;
    while (placed.some((p) => Math.abs(p.x - x(e.price)) < 110 && Math.abs(p.y - (y(e.score) + dy)) < 13)) dy += 14;
    placed.push({ x: x(e.price), y: y(e.score) + dy });
    offsets.set(e.key, dy);
  }
  const flip = (e) => x(e.price) > W - mg.r - 120;
  pts.append('text').attr('class', (e) => 'lbl' + (e.isBase ? ' base' : '') + (frontier.has(e.key) ? '' : ' dim'))
    .attr('x', (e) => (flip(e) ? -10 : 10)).attr('y', (e) => offsets.get(e.key))
    .attr('text-anchor', (e) => (flip(e) ? 'end' : 'start'))
    .text((e) => trim(e.label, 22) + (e.isBase ? ' (baseline)' : ''));
  const lines = (e) => [e.label + (e.isBase ? ' (baseline)' : ''),
    `score (summary) ${num(e.score)}`, `price ${fmtPrice(e.price)} ${unitText(e.unit)}`,
    countsText(e.counts), frontier.has(e.key) ? 'on the frontier' : 'beaten on price and score by another side'];
  pts.on('mousemove', (ev, e) => showTip(ev, lines(e))).on('mouseleave', hideTip)
    .on('focus', (ev, e) => showTip(ev, lines(e))).on('blur', hideTip);
}

function indexList(pp, frontier) {
  const d3 = window.d3;
  const sorted = [...pp.usable].sort((a, b) => b.index - a.index);
  const w = d3.scaleLinear().domain([0, d3.max(sorted, (e) => e.index)]).range([0, 100]);
  const ref = pp.usable.find((e) => e.key === pp.indexFrom);
  return h('div', { class: 'sum-price-bars' },
    h('h3', { text: 'Score per price, index' }),
    h('p', { class: 'muted', text: `${ref.isBase ? 'The baseline' : 'The cheapest side (the baseline has no usable price)'} (${ref.label}) = 100; higher is more score for the money.` }),
    h('ul', { class: 'sum-price-list' }, sorted.map((e) => h('li', {},
      h('span', { class: 'sum-price-name', text: e.label + (e.isBase ? ' (baseline)' : '') }),
      h('span', { class: 'sum-price-track', 'aria-hidden': 'true' },
        h('span', { class: 'sum-price-bar' + (frontier.has(e.key) ? ' front' : ''), style: `width:${w(e.index).toFixed(2)}%` })),
      h('span', { class: 'sum-price-val', text: num(e.index) }),
      h('span', { class: 'sum-price-sub muted', text: `score (summary) ${num(e.score)} · ${countsText(e.counts)}` })))));
}

function problemList(pp) {
  if (!pp.problems.length) return null;
  return h('div', { class: 'sum-price-problems' },
    h('h3', { text: 'Sides not in the chart' }),
    h('ul', {}, pp.problems.map((p) => h('li', {},
      h('b', { text: p.label }), ': ' + (REASONS[p.reason] || p.reason),
      p.detail ? h('span', { class: 'muted', text: ' (' + p.detail + ')' }) : null))));
}

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  const pp = pricePerformance(entriesOf(ctx));
  if (!pp.show) {
    const bad = pp.problems.filter((p) => p.reason !== 'none').map((p) => `${p.label}: ${REASONS[p.reason] || p.reason}`);
    ctx.hide(NOTE + (bad.length ? ` (${bad.join('; ')})` : ''));
    return;
  }
  const frontier = paretoFrontier(pp.usable);
  const prof = ctx.profile.profile;
  const profLabel = prof === 'custom' ? 'custom weights' : PROFILES[prof].label;
  clear(el, h('h2', { text: 'Price / performance' }),
    h('p', { class: 'muted', text: `Overall score (summary, profile: ${profLabel}) against price in ${unitText(pp.unit)}. Circles joined by the line are not beaten by any side that is both cheaper and faster; hollow squares are.` }));
  const chart = h('div', { class: 'chart sum-price-chart' });
  el.append(chart);
  drawScatter(chart, pp, frontier);
  el.append(indexList(pp, frontier));
  const probs = problemList(pp);
  if (probs) el.append(probs);
  el.append(tableFallback({
    label: 'Price / performance, one row per side',
    columns: ['Side', `Price (${unitText(pp.unit)})`, 'Score (summary)', 'Better', 'Worse', 'Within noise', 'Score per price, index', 'Frontier'],
    rows: [...pp.usable].sort((a, b) => a.price - b.price).map((e) => [e.label + (e.isBase ? ' (baseline)' : ''), fmtPrice(e.price), num(e.score),
      e.counts ? e.counts.better : '', e.counts ? e.counts.worse : '', e.counts ? e.counts.not_clear : '',
      num(e.index), frontier.has(e.key) ? 'yes' : 'no']),
  }));
}
