// UI state lives in the URL query string so a link reproduces the page:
//   view=summary|compare|metric|runs|hardware  base=REF  other=REF (repeated)  section=key|all|cpu|...
//   metric=METRIC_ID  run=RUN_ID (repeated, hardware view)  q / arch / group / tag (runs filters)
//   profile=balanced|web|db|compute|custom  w=cpu:1,disk:3,... (custom section weights; summary view)

const LISTS = ['other', 'run'];
const DEFAULTS = { view: 'summary', base: '', section: 'key', metric: '', q: '', arch: '', group: '', tag: '', profile: 'balanced', w: '' };

export function readState() {
  const p = new URLSearchParams(location.search);
  const s = {};
  for (const [k, d] of Object.entries(DEFAULTS)) s[k] = p.get(k) ?? d;
  for (const k of LISTS) s[k] = p.getAll(k).filter(Boolean);
  if (!['summary', 'compare', 'metric', 'runs', 'hardware'].includes(s.view)) s.view = 'summary';
  return s;
}

function toQuery(s) {
  const p = new URLSearchParams();
  for (const [k, d] of Object.entries(DEFAULTS)) if (s[k] && s[k] !== d) p.set(k, s[k]);
  for (const k of LISTS) for (const v of s[k] || []) p.append(k, v);
  const q = p.toString();
  return q ? '?' + q : location.pathname;
}

/** Change part of the state; push a history entry unless `replace`. Fires `statechange`. */
export function setState(patch, { replace = false } = {}) {
  const next = { ...readState(), ...patch };
  const url = toQuery(next);
  if (url !== location.search && !(url === location.pathname && !location.search)) {
    history[replace ? 'replaceState' : 'pushState'](null, '', url);
  }
  window.dispatchEvent(new Event('statechange'));
}

/** Link (query string) for a state patch, for real <a href> elements. */
export function hrefFor(patch) {
  return toQuery({ ...readState(), ...patch });
}
