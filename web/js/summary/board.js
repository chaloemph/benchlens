// Summary block: Leaderboard. Stub from the shell task: replace render() and keep the signature.

import { h, clear } from '../ui.js';

/** Fill `el` (the block's slot) from `ctx` = { comparison, runs, groups, state, profile, scores, hide } (see index.js). */
export function render(el, ctx) {
  clear(el, h('p', { class: 'muted sum-placeholder', text: 'Leaderboard: coming soon.' }));
}
