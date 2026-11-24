// Shared by the Summary blocks: the block heading and the texts every block writes the same way (score, verdict
// counts, "incomplete", profile name), so one side reads the same in every block. No maths here (score.js).

import { h } from '../ui.js';
import { sectionName } from '../format.js';
import { PROFILES } from '../score.js';

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

/** The heading of a block: the same element, level and class in every block. */
export const blockTitle = (text) => h('h2', { class: 'sum-title', text });

/** A score (summary) as text: one decimal, "no score" when there is none. */
export const fmtScore = (v) => (isNum(v) ? v.toFixed(1) : 'no score');

/** "better 23 · worse 37 · within noise 5" from `{better, worse, not_clear}`. */
export const countsText = (c) => `better ${c.better} · worse ${c.worse} · within noise ${c.not_clear}`;

/** "incomplete: no Web, Disk results" for the `incomplete` list of overallScore, or ''. */
export function incompleteText(incomplete) {
  return incomplete && incomplete.length ? 'incomplete: no ' + incomplete.map(sectionName).join(', ') + ' results' : '';
}

/** Name of a profile of weightsFor: "Balanced", "Database", ..., "Custom". */
export const profileName = (profile) => (Object.hasOwn(PROFILES, profile) ? PROFILES[profile].label : 'Custom');
