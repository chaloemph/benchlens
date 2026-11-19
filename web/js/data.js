// Runs and groups loaded from the API, side options for the pickers and quick picks derived from them.

import { api } from './api.js';
import { archName, shortDate } from './format.js';

export const store = { runs: [], groups: [], runById: new Map(), loaded: false };

export async function loadAll() {
  const [runs, groups] = await Promise.all([api.runs(), api.groups()]);
  store.runs = runs;
  store.groups = groups;
  store.runById = new Map(runs.map((r) => [r.run_id, r]));
  store.loaded = true;
}

const archRank = (a) => (a === 'x86_64' ? 0 : a === 'aarch64' ? 1 : 2);
const first = (xs) => (xs && xs.length ? xs[0] : '');

export const isMixed = (g) => g.archs.length > 1 || g.nproc.length > 1 || g.cpu_models.length > 1;

export function sortedGroups() {
  return [...store.groups].sort((a, b) =>
    archRank(first(a.archs)) - archRank(first(b.archs)) || (first(a.nproc) || 0) - (first(b.nproc) || 0) || a.group.localeCompare(b.group));
}

export function sortedRuns() {
  const order = new Map(sortedGroups().map((g, i) => [g.group, i]));
  return [...store.runs].sort((a, b) =>
    (order.get(a.group) ?? 999) - (order.get(b.group) ?? 999) || a.start_utc.localeCompare(b.start_utc) || a.label.localeCompare(b.label));
}

/** The common value of a tag over runs, or ''. */
function commonTag(runs, key) {
  const vals = new Set(runs.map((r) => r.tags[key] || ''));
  return vals.size === 1 ? [...vals][0] : '';
}

const runsOf = (group) => store.runs.filter((r) => r.group === group);

function groupSize(g) {
  return commonTag(runsOf(g.group), 'size') || (g.nproc.length === 1 ? g.nproc[0] + ' vCPU' : '');
}

/** Picker entry for a side reference (`group:NAME` or a run id). */
export function sideInfo(ref) {
  if (ref.startsWith('group:')) {
    const name = ref.slice(6);
    const g = store.groups.find((x) => x.group === name);
    if (!g) return { ref, label: name, sub: 'unknown group', known: false };
    const sub = [g.runs + (g.runs === 1 ? ' run' : ' runs'), g.archs.map(archName).join('+'),
      g.nproc.length ? g.nproc.join('/') + ' vCPU' : ''].filter(Boolean).join(' · ');
    return { ref, label: name, sub, mixed: isMixed(g), known: true, kind: 'group' };
  }
  const r = store.runById.get(ref);
  if (!r) return { ref, label: ref, sub: 'unknown run', known: false };
  const round = r.tags.round ? 'round ' + r.tags.round : '';
  return { ref, label: r.label, sub: [r.group, round, shortDate(r.start_utc)].filter(Boolean).join(' · '),
    known: true, kind: 'run', run: r };
}

export const groupRef = (g) => 'group:' + g.group;

/** Quick picks: { label, base, others[] } derived from the groups and run tags present. */
export function quickPicks() {
  const picks = [];
  const groups = sortedGroups().filter((g) => !isMixed(g) && g.archs.length === 1);

  // same size, different architecture
  const bySize = new Map();
  for (const g of groups) {
    const k = g.nproc[0] + '|' + groupSize(g);
    if (!bySize.has(k)) bySize.set(k, []);
    bySize.get(k).push(g);
  }
  for (const list of bySize.values()) {
    const archs = [...new Set(list.map((g) => g.archs[0]))];
    if (archs.length < 2) continue;
    const base = list[0];
    const others = list.filter((g) => g.archs[0] !== base.archs[0]);
    picks.push({ label: `${archName(base.archs[0])} vs ${[...new Set(others.map((g) => archName(g.archs[0])))].join(', ')} · ${groupSize(base)}`,
      base: groupRef(base), others: others.map(groupRef) });
  }

  // every size of one architecture
  const byArch = new Map();
  for (const g of groups) {
    if (!byArch.has(g.archs[0])) byArch.set(g.archs[0], []);
    byArch.get(g.archs[0]).push(g);
  }
  for (const [arch, list] of byArch) {
    if (new Set(list.map((g) => g.nproc[0])).size < 2) continue;
    picks.push({ label: `All sizes · ${archName(arch)}`, base: groupRef(list[0]), others: list.slice(1).map(groupRef) });
  }

  // everything against the biggest machine of the first architecture
  if (store.groups.length >= 3) {
    const sg = sortedGroups();
    const firstArch = first(sg[0].archs);
    const big = sg.filter((g) => first(g.archs) === firstArch).pop();
    picks.push({ label: `Everything vs ${big.group}`, base: groupRef(big), others: sg.filter((g) => g !== big).map(groupRef) });
  }

  // rounds of one group (the `round` tag), else the runs of a small group one by one
  for (const g of sortedGroups()) {
    const runs = runsOf(g.group);
    if (runs.length < 2) continue;
    const rounds = [...new Set(runs.map((r) => r.tags.round).filter(Boolean))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
    if (rounds.length >= 2) {
      // one run per round: the one with the most values (the full run, not a partial rerun), earliest on a tie
      const pick = rounds.map((rd) => runs.filter((r) => r.tags.round === rd)
        .sort((a, b) => b.values - a.values || a.start_utc.localeCompare(b.start_utc))[0]);
      picks.push({ label: `Round ${rounds.join(' vs ')} · ${g.group}`, base: pick[0].run_id, others: pick.slice(1).map((r) => r.run_id) });
    } else if (runs.length <= 8) {
      picks.push({ label: `Runs of ${g.group}`, base: runs[0].run_id, others: runs.slice(1).map((r) => r.run_id) });
    }
  }
  return picks.slice(0, 14);
}

/** A starting comparison when the URL names none. */
export function defaultComparison() {
  const qp = quickPicks();
  if (qp.length) return { base: qp[0].base, others: qp[0].others };
  const sg = sortedGroups();
  if (sg.length >= 2) return { base: groupRef(sg[0]), others: [groupRef(sg[1])] };
  const sr = sortedRuns();
  if (sr.length >= 2) return { base: sr[0].run_id, others: [sr[1].run_id] };
  return null;
}
