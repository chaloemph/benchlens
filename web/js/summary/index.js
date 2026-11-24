// Summary view: charts for decision makers, from the same comparison as the Compare view. This module only
// loads the data and hands every block its slot; the blocks are in the files next to it and the score maths
// is in ../score.js.
//
// Each block exports `render(el, ctx)` (may be async): `el` is its slot, emptied and shown before the call;
// `ctx` = { comparison, runs, groups, state, profile, scores, hide }:
// - comparison: the Comparison of /api/compare for the sides of the URL (same cache as Compare);
// - runs, groups: RunSummary[] and GroupSummary[] of /api/runs and /api/groups;
// - state: the URL state (readState), incl. `profile` and `w` as written;
// - profile: weightsFor(state.profile, state.w) = { profile, weights, invalid } (invalid custom weights or an
//   unknown profile fall back to balanced, with invalid = true);
// - scores: scoresFor(comparison, profile.weights), computed once per render;
// - hide(reason): empties and hides the slot and adds `reason` (one line, plain text) to #sum-hidden-notes.
// A block that throws gets an error box in its slot; the other blocks still render.

import { store, sideInfo } from '../data.js';
import { getComparison } from '../compare.js';
import { hrefFor, setState } from '../state.js';
import { h, clear, stateBlock, errorBlock } from '../ui.js';
import { weightsFor, scoresFor } from '../score.js';
import { render as renderBoard } from './board.js';
import { render as renderRadar } from './radar.js';
import { render as renderPrice } from './price.js';
import { render as renderScaling } from './scaling.js';
import { render as renderStability } from './stability.js';
import { render as renderVerdicts } from './verdicts.js';

// slot id and render function of each block, in page order
const BLOCKS = [
  ['sum-board', renderBoard],
  ['sum-radar', renderRadar],
  ['sum-price', renderPrice],
  ['sum-scaling', renderScaling],
  ['sum-stability', renderStability],
  ['sum-verdicts', renderVerdicts],
];

let renderSeq = 0;

/** The sides line: names the sides and links to Compare (same state) where they are changed. */
function sidesLine(s) {
  const el = document.getElementById('sum-sides');
  const name = (ref) => sideInfo(ref).label;
  const link = h('a', { href: hrefFor({ view: 'compare' }), text: 'change' });
  link.addEventListener('click', (e) => {
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
    e.preventDefault();
    setState({ view: 'compare' });
  });
  if (!s.base || !s.other.length) return clear(el, s.base ? 'Sides: pick at least one side to compare with ' + name(s.base) + ' · ' : 'Sides: none picked · ', link);
  return clear(el, 'Sides: ', h('b', { text: name(s.base) }), ' (baseline) against ', s.other.map(name).join(', '), ' · ', link);
}

export function renderSummary(s) {
  const seq = ++renderSeq;
  sidesLine(s);
  const status = document.getElementById('sum-status');
  const body = document.getElementById('sum-body');
  if (!s.base || !s.other.length) {
    body.hidden = true;
    clear(status, stateBlock('empty', s.base ? 'Pick at least one group or run to compare with the baseline.' : 'Pick a baseline.'));
    return;
  }
  clear(status, stateBlock('loading', 'Comparing…'));
  getComparison(s.base, s.other).then(async (comparison) => {
    if (seq !== renderSeq) return;
    clear(status);
    body.hidden = false;
    const notes = clear(document.getElementById('sum-hidden-notes'));
    const profile = weightsFor(s.profile, s.w);
    const scores = scoresFor(comparison, profile.weights);
    for (const [id, render] of BLOCKS) {
      const el = clear(document.getElementById(id));
      el.hidden = false;
      let hidden = false;
      const hide = (reason) => {
        if (hidden || seq !== renderSeq) return; // once per block, never from a superseded render
        hidden = true;
        clear(el);
        el.hidden = true;
        notes.append(h('span', { class: 'sum-note', text: reason }), ' ');
      };
      try {
        await render(el, { comparison, runs: store.runs, groups: store.groups, state: s, profile, scores, hide });
      } catch (err) {
        if (seq === renderSeq) clear(el, errorBlock(err, 'Cannot draw this block'));
      }
      if (seq !== renderSeq) return;
    }
  }).catch((err) => {
    if (seq !== renderSeq) return;
    body.hidden = true;
    clear(status, errorBlock(err, 'Cannot compare'));
  });
}
