// Summary block: Leaderboard with workload profiles. One bar per side (baseline included, at 100), sorted by
// the overall score of the chosen profile, plus the profile control. The scores come from score.js through
// ctx.scores; verdict counts come from the comparison's `tallies`. No score maths here.

import { h, clear } from '../ui.js';
import { setState } from '../state.js';
import { PROFILES, SECTIONS, formatWeights, checkWeights } from '../score.js';
import { tableFallback } from './table.js';

const SECTION_NAMES = { cpu: 'CPU', disk: 'disk', mem: 'memory', net: 'network', pg: 'PostgreSQL', web: 'web' };
const PROFILE_ORDER = ['balanced', 'web', 'db', 'compute', 'custom'];
const profileLabel = (p) => (p === 'custom' ? 'Custom' : PROFILES[p].label);
const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

/** Score as text: one decimal close to the baseline (where whole numbers would hide the gap), else whole. */
export function fmtScore(v) {
  if (!isNum(v)) return 'no score';
  return window.d3.format(Math.abs(v - 100) < 10 ? '.1f' : '.0f')(v);
}

/** "incomplete: no web, disk results" or '' */
export function incompleteText(overall) {
  const miss = overall && overall.incomplete ? overall.incomplete : [];
  return miss.length ? 'incomplete: no ' + miss.map((s) => SECTION_NAMES[s] || s).join(', ') + ' results' : '';
}

/**
 * Entries of the board, best first (no score last, ties keep the URL order, the baseline before the others
 * on a tie): { name, baseline, score, overall, counts | null }.
 */
export function boardEntries(ctx) {
  const { comparison, scores } = ctx;
  const entries = [{ name: scores.base.label, baseline: true, score: scores.base.overall.score, overall: scores.base.overall, counts: null }];
  scores.sides.forEach((s, i) => {
    const t = (comparison.tallies || [])[i] || {};
    entries.push({
      name: s.label, baseline: false, score: s.overall.score, overall: s.overall,
      counts: { better: t.better || 0, worse: t.worse || 0, not_clear: t.not_clear || 0 },
    });
  });
  return entries
    .map((e, i) => ({ e, i }))
    .sort((a, b) => (isNum(b.e.score) ? b.e.score : -Infinity) - (isNum(a.e.score) ? a.e.score : -Infinity) || a.i - b.i)
    .map((x) => x.e);
}

/** The one-line takeaway above the bars. */
export function takeaway(entries, profileName) {
  const base = entries.find((e) => e.baseline);
  const lead = entries[0];
  const others = entries.filter((e) => !e.baseline && isNum(e.score));
  if (!others.length) return `For ${profileName}, there is no score to rank: the compared sides have no results this profile counts.`;
  if (!lead.baseline) return `For ${profileName}, ${lead.name} scores ${fmtScore(lead.score)} against ${base.name} (100).`;
  return `For ${profileName}, ${base.name} (100) leads; the best of the others, ${others[0].name}, scores ${fmtScore(others[0].score)}.`;
}

// Focus across re-renders. Changing the profile or a weight calls setState, which re-renders every block and
// replaces the content of this slot, so the control the user was on would lose the keyboard focus. The board
// remembers the last control focused inside it and, after a re-render it caused itself, gives the focus back to
// that control (the new element with the same id). The custom weight inputs are kept as they are while the URL
// holds the weights they produced, so a value being typed in another input survives the re-render too.
const tracked = new WeakSet();
let lastFocus = null;   // last element focused inside the slot
let refocus = false;    // the next render follows a change made with the board's own controls
let kept = null;        // { w, node }: the custom inputs and the `w` they stand for

function trackFocus(el) {
  if (tracked.has(el)) return;
  tracked.add(el);
  el.addEventListener('focusin', (e) => { lastFocus = e.target; });
  el.addEventListener('focusout', (e) => { if (e.relatedTarget && !el.contains(e.relatedTarget)) lastFocus = null; });
}

// A change event can fire in the middle of a focus move (Tab out of a weight input): the state is set one task
// later, once the focus has landed, so the re-render gives it back to where it went, not where it was.
function change(patch) {
  refocus = true;
  setTimeout(() => setState(patch), 0);
}

function restoreFocus(el) {
  if (!refocus) return;
  refocus = false;
  const active = document.activeElement;
  if (!lastFocus || (active && active !== document.body)) return; // the user is elsewhere already
  const target = lastFocus.isConnected ? lastFocus : (lastFocus.id ? el.querySelector('#' + CSS.escape(lastFocus.id)) : null);
  if (target && el.contains(target)) target.focus({ preventScroll: true });
}

function profileControl(ctx) {
  const { state, profile } = ctx;
  const chosen = state.profile === 'custom' ? 'custom' : profile.profile;
  const group = h('div', { class: 'sum-board-profiles', role: 'radiogroup', 'aria-label': 'Workload profile' });
  for (const p of PROFILE_ORDER) {
    const input = h('input', { type: 'radio', name: 'sum-profile', value: p, checked: chosen === p, id: 'sum-profile-' + p });
    input.addEventListener('change', () => {
      if (!input.checked) return;
      if (p === 'custom') change({ profile: 'custom', w: formatWeights(profile.weights) });
      else change({ profile: p, w: '' });
    });
    group.append(h('label', { class: 'sum-board-profile' + (chosen === p ? ' on' : '') }, input, ' ', profileLabel(p)));
  }
  const box = h('div', { class: 'sum-board-control' }, h('div', { class: 'muted', text: 'Workload profile (weights the six sections)' }), group);
  if (state.profile === 'custom' && profile.invalid) {
    box.append(h('p', { class: 'sum-board-error', role: 'alert', text: 'The custom weights in this link are not valid, so Balanced is shown. Enter new weights below.' }));
  } else if (state.profile && state.profile !== 'custom' && profile.invalid) {
    box.append(h('p', { class: 'sum-board-error', role: 'alert', text: 'Unknown profile "' + state.profile + '", so Balanced is shown.' }));
  }
  if (chosen === 'custom') {
    if (!kept || kept.w !== state.w || profile.invalid) kept = { w: state.w, node: customInputs(profile.weights) };
    box.append(kept.node);
  } else {
    kept = null;
  }
  return box;
}

function customInputs(weights) {
  const inputs = {};
  const err = h('p', { class: 'sum-board-error', role: 'alert', hidden: true });
  const grid = h('div', { class: 'sum-board-weights' });
  const apply = () => {
    const texts = Object.fromEntries(SECTIONS.map((s) => [s, inputs[s].value]));
    const res = checkWeights(texts);
    for (const s of SECTIONS) inputs[s].setAttribute('aria-invalid', String(res.section === s));
    if (res.error) {
      err.textContent = res.section ? `${SECTION_NAMES[res.section]}: ${res.error}.` : res.error[0].toUpperCase() + res.error.slice(1) + '.';
      err.hidden = false;
      return;
    }
    err.hidden = true;
    err.textContent = '';
    const w = formatWeights(res.weights);
    if (kept && kept.node === box) kept.w = w;
    change({ profile: 'custom', w });
  };
  for (const sec of SECTIONS) {
    const input = h('input', { type: 'number', min: '0', max: '5', step: '0.5', value: String(weights[sec]), id: 'sum-w-' + sec });
    input.addEventListener('change', apply);
    inputs[sec] = input;
    grid.append(h('label', { class: 'sum-board-weight', for: 'sum-w-' + sec }, h('span', { text: SECTION_NAMES[sec] }), input));
  }
  const box = h('div', { class: 'sum-board-custom' }, grid, err);
  return box;
}

function bars(entries, profileName) {
  const d3 = window.d3;
  const top = d3.max(entries, (e) => (isNum(e.score) ? e.score : 0)) || 100;
  const max = Math.max(120, Math.ceil(top / 20) * 20);
  const x = d3.scaleLinear().domain([0, max]).range([0, 100]);
  const list = h('ol', { class: 'sum-board-list', 'aria-label': `Overall score (summary) for ${profileName}, baseline = 100` });
  entries.forEach((e) => {
    const pct = isNum(e.score) ? x(e.score) : 0;
    const bar = h('div', { class: 'sum-board-track', 'aria-hidden': 'true' },
      isNum(e.score) ? h('div', { class: 'sum-board-bar' + (e.baseline ? ' base' : ''), style: `width:${pct}%` }) : null,
      h('div', { class: 'sum-board-100', style: `left:${x(100)}%` }));
    const detail = e.baseline
      ? 'baseline'
      : `better ${e.counts.better} · worse ${e.counts.worse} · within noise ${e.counts.not_clear}`;
    const inc = incompleteText(e.overall);
    list.append(h('li', { class: 'sum-board-row', 'data-baseline': e.baseline ? 'true' : null },
      h('div', { class: 'sum-board-head' },
        h('span', { class: 'sum-board-name', text: e.name + (e.baseline ? ' (baseline)' : '') }),
        h('span', { class: 'sum-board-value', text: fmtScore(e.score) })),
      bar,
      h('div', { class: 'sum-board-detail muted', text: detail }),
      inc ? h('div', { class: 'sum-board-incomplete', text: inc }) : null));
  });
  return list;
}

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  trackFocus(el);
  const name = profileLabel(ctx.profile.profile);
  const entries = boardEntries(ctx);
  clear(el,
    h('h2', { text: 'Leaderboard' }),
    h('p', { class: 'lede sum-board-takeaway', text: takeaway(entries, name) }),
    profileControl(ctx),
    h('p', { class: 'muted', text: `Overall score (summary) for ${name}: weighted geometric mean of the section scores, baseline = 100. A score is a summary, not a verdict; the counts below each bar are vmbench's verdicts.` }),
    bars(entries, name),
    tableFallback({
      label: 'Leaderboard as a table',
      columns: ['Side', 'Overall score (summary)', 'Better', 'Worse', 'Within noise', 'Note'],
      rows: entries.map((e) => [
        e.name + (e.baseline ? ' (baseline)' : ''), fmtScore(e.score),
        e.counts ? e.counts.better : '', e.counts ? e.counts.worse : '', e.counts ? e.counts.not_clear : '',
        e.baseline ? 'baseline' : incompleteText(e.overall),
      ]),
    }));
  restoreFocus(el);
}
