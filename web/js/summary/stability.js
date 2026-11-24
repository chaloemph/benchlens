// Summary block: Stability. A Cleveland dot chart: one row per section, one dot per side (the baseline
// included) at the median CV in %, lower is better, x from 0. A shaded band marks the median `noise` threshold of
// the section, so a dot inside it is as steady as the verdict rule's own noise. The CVs and the noise band come
// from score.js (ctx.scores.base / sides[].stability, ctx.scores.noise = medianNoise); this file only draws them. Sides whose CVs are all null were measured once and are listed,
// never drawn at 0.

import { h, clear, showTip, hideTip } from '../ui.js';
import { SECTIONS } from '../score.js';
import { sectionName, pct1 } from '../format.js';
import { tableFallback } from './table.js';

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

/** The sides as chart series: baseline first, with `cv` per section and whether it was measured once. */
export function seriesOf(scores) {
  const mk = (label, cv, sections, isBase) => ({
    label, cv, sections, isBase,
    measuredOnce: !SECTIONS.some((s) => isNum(cv[s])),
  });
  return [mk(scores.base.label, scores.base.stability, null, true),
    ...scores.sides.map((s) => mk(s.label, s.stability, s.sections, false))];
}

const counts = (c) => `better ${c.better} · worse ${c.worse} · within noise ${c.not_clear}`;

/** Tooltip / table text of one dot. */
function describe(series, sec) {
  const parts = [pct1(series.cv[sec])];
  if (series.sections) {
    const s = series.sections[sec];
    parts.push(`${s.n} results`, counts(s.counts));
  }
  return parts;
}

const SYMBOLS = ['symbolCircle', 'symbolSquare', 'symbolTriangle', 'symbolCross', 'symbolStar', 'symbolWye'];
const symbolFor = (i) => (i === 0 ? 'symbolDiamond' : SYMBOLS[(i - 1) % SYMBOLS.length]);
const slotClass = (i) => (i === 0 ? 'sum-stab-base' : 'sum-stab-c' + ((i - 1) % 6));

export function render(el, ctx) {
  const d3 = window.d3;
  const { scores } = ctx;
  const series = seriesOf(scores);
  const noise = scores.noise;
  const drawn = series.filter((s) => !s.measuredOnce);
  const once = series.filter((s) => s.measuredOnce);
  const secs = SECTIONS.filter((sec) => drawn.some((s) => isNum(s.cv[sec])));

  // the noisiest cell
  let top = null;
  for (const s of drawn) for (const sec of secs) if (isNum(s.cv[sec]) && (!top || s.cv[sec] > top.v)) top = { s, sec, v: s.cv[sec] };

  const head = [h('h2', { text: 'Stability' })];
  head.push(h('p', { class: 'sum-stab-lead', text: top
    ? `Noisiest: ${top.s.label}${top.s.isBase ? ' (baseline)' : ''} in ${sectionName(top.sec)}, median CV ${pct1(top.v)}.`
    : 'No side has repeated measurements, so there is no stability data to compare.' }));
  head.push(h('p', { class: 'muted', text: 'Median CV (spread between repeated runs) of the results behind each section score; lower is steadier. Shaded band: median noise threshold of the section. Summary only, verdicts are unchanged.' }));
  clear(el, head);

  if (secs.length) {
    const W = Math.max(280, el.clientWidth || 600);
    const lane = 15;
    const rowH = drawn.length * lane + 14;
    const wrap = h('div', { class: 'chart sum-stab-wrap' });
    el.append(wrap);
    const svg = d3.select(wrap).append('svg').attr('class', 'sum-stab-chart').attr('role', 'img')
      .attr('aria-label', 'Median CV in percent per section and side, lower is steadier');
    const probe = svg.append('text').attr('class', 'lbl');
    let lw = 0;
    for (const sec of secs) { probe.text(sectionName(sec)); lw = Math.max(lw, probe.node().getComputedTextLength()); }
    probe.remove();
    const mg = { l: Math.min(Math.ceil(lw) + 12, Math.floor(W * 0.3)), r: 14, t: 8, b: 34 };
    const H = mg.t + secs.length * rowH + mg.b;
    svg.attr('viewBox', `0 0 ${W} ${H}`).attr('width', W).attr('height', H);
    const maxV = d3.max([...drawn.flatMap((s) => secs.map((sec) => s.cv[sec])), ...secs.map((sec) => noise[sec])].filter(isNum)) || 1;
    const x = d3.scaleLinear().domain([0, maxV * 1.08]).range([mg.l, W - mg.r]).nice();
    const ticks = Math.max(2, Math.floor((W - mg.l - mg.r) / 70));
    svg.append('g').attr('class', 'grid').selectAll('line').data(x.ticks(ticks)).join('line')
      .attr('x1', x).attr('x2', x).attr('y1', mg.t).attr('y2', H - mg.b);
    svg.append('g').attr('class', 'axis').attr('transform', `translate(0,${H - mg.b})`)
      .call(d3.axisBottom(x).ticks(ticks).tickFormat((v) => v + '%')).call((g) => g.select('.domain').remove());
    svg.append('text').attr('class', 'lbl sum-stab-axis').attr('x', (mg.l + W - mg.r) / 2).attr('y', H - 4)
      .attr('text-anchor', 'middle').text('Median CV (%), lower is steadier');

    secs.forEach((sec, k) => {
      const top0 = mg.t + k * rowH;
      if (isNum(noise[sec])) {
        svg.append('rect').attr('class', 'sum-stab-band').attr('x', x(0)).attr('y', top0 + 1)
          .attr('width', Math.max(0, x(noise[sec]) - x(0))).attr('height', rowH - 2)
          .append('title').text(`${sectionName(sec)}: within noise up to ${pct1(noise[sec])}`);
        svg.append('line').attr('class', 'sum-stab-bandedge').attr('x1', x(noise[sec])).attr('x2', x(noise[sec]))
          .attr('y1', top0 + 1).attr('y2', top0 + rowH - 1);
      }
      svg.append('text').attr('class', 'lbl').attr('x', 0).attr('y', top0 + rowH / 2 + 4).text(sectionName(sec));
      svg.append('line').attr('class', 'sum-stab-sep').attr('x1', 0).attr('x2', W).attr('y1', top0 + rowH).attr('y2', top0 + rowH);
      drawn.forEach((s, i) => {
        if (!isNum(s.cv[sec])) return;
        const y = top0 + 7 + lane * i + lane / 2;
        const sym = d3.symbol(d3[symbolFor(series.indexOf(s))], 70);
        const text = `${s.label}${s.isBase ? ' (baseline)' : ''}: ${sectionName(sec)}, median CV ${describe(s, sec).join(', ')}`;
        const dot = svg.append('path').attr('class', 'sum-stab-dot ' + slotClass(series.indexOf(s)))
          .attr('d', sym()).attr('transform', `translate(${x(s.cv[sec])},${y})`)
          .attr('tabindex', 0).attr('role', 'img').attr('aria-label', text);
        const lines = [`${s.label}${s.isBase ? ' (baseline)' : ''}`, `${sectionName(sec)}: median CV ${pct1(s.cv[sec])}`, ...describe(s, sec).slice(1)];
        dot.on('mousemove', (ev) => showTip(ev, lines)).on('mouseleave', hideTip)
          .on('focus', (ev) => showTip(ev, lines)).on('blur', hideTip);
      });
    });

    // legend: marker shape + line style as well as colour
    const legend = h('div', { class: 'legend sum-stab-legend' });
    series.forEach((s, i) => {
      if (s.measuredOnce) return;
      const sw = d3.create('svg').attr('width', 14).attr('height', 14).attr('viewBox', '-7 -7 14 14').attr('aria-hidden', 'true');
      sw.append('path').attr('class', 'sum-stab-dot ' + slotClass(i)).attr('d', d3.symbol(d3[symbolFor(i)], 60)());
      legend.append(h('span', {}, sw.node(), s.label + (s.isBase ? ' (baseline)' : '')));
    });
    const band = d3.create('svg').attr('width', 18).attr('height', 14).attr('aria-hidden', 'true');
    band.append('rect').attr('class', 'sum-stab-band').attr('x', 0).attr('y', 1).attr('width', 15).attr('height', 12);
    band.append('line').attr('class', 'sum-stab-bandedge').attr('x1', 15).attr('x2', 15).attr('y1', 1).attr('y2', 13);
    legend.append(h('span', {}, band.node(), 'within noise'));
    el.append(legend);
  }

  if (once.length) {
    el.append(h('ul', { class: 'sum-stab-once' }, ...once.map((s) =>
      h('li', { text: `${s.label}${s.isBase ? ' (baseline)' : ''}: measured once: no stability data` }))));
  }

  const cell = (s, sec) => {
    if (s.measuredOnce) return 'measured once';
    return isNum(s.cv[sec]) ? describe(s, sec).filter((_, i) => i !== 1).join(', ') : 'no data';
  };
  el.append(tableFallback({
    label: 'Median CV in percent per section and side',
    columns: ['Section', 'Noise band (median threshold)', ...series.map((s) => `${s.label}${s.isBase ? ' (baseline)' : ''}: median CV`)],
    rows: SECTIONS.map((sec) => [sectionName(sec), isNum(noise[sec]) ? pct1(noise[sec]) : 'no data', ...series.map((s) => cell(s, sec))]),
  }));
}
