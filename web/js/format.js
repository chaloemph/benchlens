// Number and label formatting. No comparison maths here: medians, CV, diff and verdicts come from the API.

const d3 = () => window.d3;

export const SECTION_NAMES = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', pg: 'PostgreSQL', net: 'Network', web: 'Web', burnin: 'Burn-in' };
const SECTION_ORDER = ['cpu', 'mem', 'disk', 'pg', 'net', 'web', 'burnin'];
export const sectionName = (s) => SECTION_NAMES[s] || s;
export function sortSections(list) {
  const rank = (s) => { const i = SECTION_ORDER.indexOf(s); return i < 0 ? 99 : i; };
  return [...list].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

const ARCH = { x86_64: 'x86', aarch64: 'ARM64' };
export const archName = (a) => ARCH[a] || a || '?';

/** A value in its unit, with sensible precision. */
export function fmt(v) {
  if (v == null || !isFinite(v)) return '—';
  const a = Math.abs(v), f = d3().format;
  return a >= 1000 ? f(',.0f')(v) : a >= 100 ? f('.0f')(v) : a >= 10 ? f('.1f')(v) : f('.3~g')(v);
}

/** A difference in percent: +12%, -3.4%. */
export function pct(d) {
  if (d == null || !isFinite(d)) return '—';
  const f = d3().format;
  return (d > 0 ? '+' : d < 0 ? '−' : '') + (Math.abs(d) >= 10 ? f('.0f')(Math.abs(d)) : f('.1f')(Math.abs(d))) + '%';
}

export const pct1 = (v) => (v == null || !isFinite(v) ? 'n/a' : d3().format('.1f')(v) + '%');

export const VERDICT_TEXT = { better: 'better', worse: 'worse', not_clear: 'within noise' };

export function shortDate(utc) {
  if (!utc) return '—';
  return utc.replace('T', ' ').replace(/:\d\dZ$/, 'Z').replace('Z', ' UTC');
}
