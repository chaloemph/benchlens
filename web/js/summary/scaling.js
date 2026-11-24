// Summary block: Scaling. "If we pay for twice the vCPUs, do we get twice the speed?"
// x = vCPU count of a side (score.js: sideVcpu), y = overall score of the side (current profile, baseline 100),
// one line per architecture, a dotted linear-scaling reference from the smallest point of each architecture.
// Hidden unless some architecture has at least two sides with different vCPU counts.
//
// The scores come from ctx.scores and the efficiency (score / linear expectation) from scalingEfficiency, both
// in score.js; this file only places them.

import { h, clear, showTip, hideTip } from '../ui.js';
import { sideVcpu, scalingEfficiency } from '../score.js';
import { tableFallback } from './table.js';
import { blockTitle, fmtScore, countsText, incompleteText as missing, profileName } from './common.js';

const HIDE_REASON = 'Scaling: pick sides of one architecture with different vCPU counts (for example a 2 and a 16 vCPU machine) to see it';
const SYMBOLS = ['Circle', 'Square', 'Triangle', 'Diamond'];
const DASHES = ['', '7 3', '2 3', '10 3 2 3'];
const SERIES = DASHES.length;
const LINEAR_DASH = '1 4';

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

/**
 * The points and lines of the chart, no DOM. Sides are the baseline (score 100) and every compared side.
 * `{points, skipped, arches}`: `points` = `{key, label, arch, vcpu, score, isBase, incomplete, counts}`;
 * `skipped` = `{label, reason}` (mixed vCPU, no vCPU, mixed arch, no score); `arches` = `{arch, points (by vCPU),
 * linear}` sorted by name, `linear` true when the arch has two different vCPU counts (then each point also has
 * `expected` and `efficiency` in %, 100 for the reference point).
 */
export function scalingModel(comparison, runs, groups, scores) {
  const sides = [
    { side: comparison.base, isBase: true, score: 100, incomplete: [], counts: null },
    ...(comparison.others || []).map((side, i) => ({
      side, isBase: false, score: scores.sides[i].overall.score, incomplete: scores.sides[i].overall.incomplete,
      counts: comparison.tallies && comparison.tallies[i] ? comparison.tallies[i] : null,
    })),
  ];
  const points = [];
  const skipped = [];
  for (const s of sides) {
    const label = s.side.label;
    const archs = s.side.archs || [];
    if (archs.length !== 1) { skipped.push({ label, reason: archs.length ? 'mixed architectures: ' + archs.join(', ') : 'architecture unknown' }); continue; }
    const v = sideVcpu(s.side, runs, groups);
    if (v.error) { skipped.push({ label, reason: v.error === 'mixed' ? 'mixed vCPU (' + v.detail + ')' : 'no vCPU count known' }); continue; }
    if (!isNum(s.score)) { skipped.push({ label, reason: 'no score' }); continue; }
    points.push({ key: s.side.reference, label, arch: archs[0], vcpu: v.vcpu, score: s.score, isBase: s.isBase, incomplete: s.incomplete, counts: s.counts });
  }
  const arches = [...new Set(points.map((p) => p.arch))].sort().map((arch) => {
    const pts = points.filter((p) => p.arch === arch).sort((a, b) => a.vcpu - b.vcpu);
    const linear = new Set(pts.map((p) => p.vcpu)).size >= 2;
    if (linear) for (const p of pts) { const e = scalingEfficiency(p, pts[0]); p.expected = e.expected; p.efficiency = e.percent; }
    return { arch, points: pts, linear };
  });
  return { points, skipped, arches };
}

const fmt1 = (v) => (isNum(v) ? fmtScore(v) : '');
const fmtPct = (v) => (isNum(v) ? v.toFixed(0) + '%' : '');
const incompleteText = (p) => missing(p.incomplete);

function tipLines(p, profileName) {
  const lines = [p.label + (p.isBase ? ' (baseline)' : ''), `${p.arch}, ${p.vcpu} vCPU`, `score (summary): ${fmt1(p.score)}${p.isBase ? ' (baseline = 100)' : ''}`];
  if (p.counts) lines.push(countsText(p.counts));
  if (p.efficiency != null) lines.push(`linear expectation ${fmt1(p.expected)}, efficiency ${fmtPct(p.efficiency)}`);
  const inc = incompleteText(p);
  if (inc) lines.push(inc);
  lines.push('profile: ' + profileName);
  return lines;
}

function legend(model) {
  const d3 = window.d3;
  const items = model.arches.map((a, i) => {
    const sw = d3.create('svg').attr('width', 38).attr('height', 14).attr('aria-hidden', 'true').attr('class', 'sum-sc-s' + (i % SERIES));
    sw.append('line').attr('x1', 1).attr('x2', 37).attr('y1', 7).attr('y2', 7).attr('class', 'sum-sc-line').attr('stroke-dasharray', DASHES[i % SERIES]);
    sw.append('path').attr('transform', 'translate(19,7)').attr('class', 'sum-sc-mark').attr('d', d3.symbol(d3['symbol' + SYMBOLS[i % SERIES]], 50)());
    return h('span', {}, sw.node(), a.arch);
  });
  const lin = d3.create('svg').attr('width', 38).attr('height', 14).attr('aria-hidden', 'true');
  lin.append('line').attr('x1', 1).attr('x2', 37).attr('y1', 7).attr('y2', 7).attr('class', 'sum-sc-ref').attr('stroke-dasharray', LINEAR_DASH);
  items.push(h('span', {}, lin.node(), 'linear scaling from the smallest size'));
  items.push(h('span', {}, 'hollow marker: baseline'));
  return h('div', { class: 'legend sum-sc-legend' }, ...items);
}

let clipCounter = 0;

function draw(box, model, profileName) {
  const d3 = window.d3;
  box.replaceChildren();
  const W = Math.max(280, Math.floor(box.clientWidth || 600));
  const H = W < 480 ? 300 : 340;
  const mg = { l: 46, r: 16, t: 16, b: 46 };
  const svg = d3.select(box).append('svg').attr('role', 'img').attr('viewBox', `0 0 ${W} ${H}`).attr('width', W).attr('height', H)
    .attr('aria-label', 'Score (summary) against vCPU count, one line per architecture');

  const vcpus = [...new Set(model.points.map((p) => p.vcpu))].sort((a, b) => a - b);
  const xmax = vcpus[vcpus.length - 1] * 1.08;
  const ymax = Math.max(100, d3.max(model.points, (p) => p.score)) * 1.18;
  const x = d3.scaleLinear().domain([0, xmax]).range([mg.l, W - mg.r]);
  const y = d3.scaleLinear().domain([0, ymax]).range([H - mg.b, mg.t]).nice();
  const yTop = y.domain()[1];

  svg.append('g').attr('class', 'grid').selectAll('line').data(y.ticks(5)).join('line')
    .attr('x1', mg.l).attr('x2', W - mg.r).attr('y1', y).attr('y2', y);
  // ticks at the real vCPU counts only; a label that would touch the previous one is left out (the table has it)
  let lastX = -Infinity;
  const tickValues = vcpus.filter((v) => { const px = x(v); if (px - lastX < 26) return false; lastX = px; return true; });
  svg.append('g').attr('class', 'axis').attr('transform', `translate(0,${H - mg.b})`)
    .call(d3.axisBottom(x).tickValues(tickValues).tickFormat(d3.format('d')));
  svg.append('g').attr('class', 'axis').attr('transform', `translate(${mg.l},0)`)
    .call(d3.axisLeft(y).ticks(5)).call((g) => g.select('.domain').remove());
  svg.append('text').attr('x', (mg.l + W - mg.r) / 2).attr('y', H - 8).attr('text-anchor', 'middle').text('vCPU');
  svg.append('text').attr('transform', `translate(12,${(mg.t + H - mg.b) / 2}) rotate(-90)`).attr('text-anchor', 'middle').text('Score (summary)');
  svg.append('line').attr('class', 'sum-sc-base').attr('x1', mg.l).attr('x2', W - mg.r).attr('y1', y(100)).attr('y2', y(100));
  svg.append('text').attr('class', 'sum-sc-baselbl').attr('x', W - mg.r - 2).attr('y', y(100) - 4).attr('text-anchor', 'end').text('baseline = 100');

  const clipId = 'sum-sc-clip-' + (++clipCounter);
  svg.append('clipPath').attr('id', clipId).append('rect').attr('x', mg.l).attr('y', mg.t).attr('width', W - mg.l - mg.r).attr('height', H - mg.t - mg.b);

  const taken = [];
  model.arches.forEach((a, i) => {
    const g = svg.append('g').attr('class', 'sum-sc-s' + (i % SERIES));
    const ref = a.points[0];
    if (a.linear) {
      // dotted reference from the smallest point, to the edge of the plot (clipped where it leaves the top)
      const slope = ref.score / ref.vcpu;
      g.append('line').attr('class', 'sum-sc-ref').attr('clip-path', `url(#${clipId})`).attr('stroke-dasharray', LINEAR_DASH)
        .attr('x1', x(ref.vcpu)).attr('y1', y(ref.score)).attr('x2', x(xmax)).attr('y2', y(slope * xmax));
      const exit = Math.min(xmax, yTop / slope);
      g.append('text').attr('class', 'sum-sc-linlbl').attr('x', x(exit) - 4).attr('y', y(Math.min(yTop, slope * exit)) + 12)
        .attr('text-anchor', 'end').text('linear');
      g.append('path').attr('class', 'sum-sc-line').attr('stroke-dasharray', DASHES[i % SERIES]).attr('fill', 'none')
        .attr('d', d3.line().x((p) => x(p.vcpu)).y((p) => y(p.score))(a.points));
    }
    const sym = d3.symbol(d3['symbol' + SYMBOLS[i % SERIES]], 90)();
    g.selectAll('path.sum-sc-mark').data(a.points).join('path')
      .attr('class', (p) => 'sum-sc-mark' + (p.isBase ? ' is-base' : ''))
      .attr('transform', (p) => `translate(${x(p.vcpu)},${y(p.score)})`).attr('d', sym)
      .attr('tabindex', 0).attr('role', 'img')
      .attr('aria-label', (p) => tipLines(p, profileName).join('; '))
      .on('pointerenter pointermove focus', (ev, p) => showTip(ev, tipLines(p, profileName)))
      .on('pointerleave blur', hideTip);
    g.selectAll('text.sum-sc-ptlbl').data(a.points).join('text')
      .attr('class', 'sum-sc-ptlbl')
      .attr('text-anchor', (p) => (x(p.vcpu) > W * 0.62 ? 'end' : 'start'))
      .attr('x', (p) => x(p.vcpu) + (x(p.vcpu) > W * 0.62 ? -9 : 9))
      .attr('y', (p) => {
        // stagger labels of points that sit close together
        let dy = -9;
        const px = x(p.vcpu), py = y(p.score);
        while (taken.some((t) => Math.abs(t.x - px) < 60 && Math.abs(t.y - (py + dy)) < 12)) dy -= 12;
        taken.push({ x: px, y: py + dy });
        return py + dy;
      })
      .text((p) => p.label + (p.isBase ? ' (baseline)' : ''));
  });
}

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  const model = scalingModel(ctx.comparison, ctx.runs, ctx.groups, ctx.scores);
  if (!model.arches.some((a) => a.linear)) { ctx.hide(HIDE_REASON); return; }

  const profile = profileName(ctx.profile.profile);
  const box = h('div', { class: 'chart sum-sc-chart' });
  const lines = model.arches.filter((a) => a.linear).map((a) => {
    const first = a.points[0];
    const last = a.points[a.points.length - 1];
    return h('li', { text: `${a.arch}: ${first.label} (${first.vcpu} vCPU, score ${fmt1(first.score)}) to ${last.label} (${last.vcpu} vCPU, score ${fmt1(last.score)}): ${fmtPct(last.efficiency)} of linear scaling.` });
  });
  const rows = model.arches.flatMap((a) => a.points.map((p) => {
    const c = p.counts;
    return [p.label + (p.isBase ? ' (baseline)' : ''), a.arch, p.vcpu, fmt1(p.score), p.efficiency != null ? fmt1(p.expected) : '', p.efficiency != null ? fmtPct(p.efficiency) : '',
      c ? c.better : '', c ? c.worse : '', c ? c.not_clear : '', incompleteText(p)];
  }));

  clear(el,
    blockTitle('Scaling with vCPUs'),
    h('p', { class: 'muted', text: `Score (summary) of each side against its vCPU count, profile ${profile}, baseline = 100. A dotted line shows what perfectly linear scaling from the smallest size would give.` }),
    h('ul', { class: 'sum-sc-notes' }, ...lines),
    box,
    legend(model),
    model.skipped.length ? h('p', { class: 'muted sum-sc-skipped', text: 'Not shown: ' + model.skipped.map((s) => `${s.label} (${s.reason})`).join('; ') + '.' }) : null,
    tableFallback({
      label: 'Scaling: score and efficiency by side',
      columns: ['Side', 'Architecture', 'vCPU', 'Score (summary)', 'Linear expectation', 'Efficiency', 'Better', 'Worse', 'Within noise', 'Note'],
      rows,
    }));

  draw(box, model, profile);
  // redraw when the slot changes width (window resize, rotation); stops once the chart is replaced
  if (typeof ResizeObserver === 'function') {
    let width = box.clientWidth;
    const ro = new ResizeObserver(() => {
      if (!box.isConnected) { ro.disconnect(); return; }
      if (Math.abs(box.clientWidth - width) < 8) return;
      width = box.clientWidth;
      draw(box, model, profile);
    });
    ro.observe(box);
  }
}
