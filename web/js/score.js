// Scores of the Summary view: pure functions (no DOM, no fetch) over a Comparison as `/api/compare` returns it,
// the runs of `/api/runs` and the groups of `/api/groups`. Scores are summaries, not verdicts: they use only
// rows[].base, value, direction, cv_*, noise, cross_arch and verdict, and never re-judge a metric. tools/score_check.py
// recomputes them independently; tests/score/ checks both agree.
//
// Section score: rows of one side and section with cross_arch == false, base > 0 and value > 0; ratio
// r = value / base (direction "higher") or base / value ("lower"), so r > 1 always means better than the
// baseline; score = 100 x geometric mean of r. Overall score: weighted geometric mean of the section scores
// that are not null, weights renormalised.

export const SECTIONS = ['cpu', 'disk', 'mem', 'net', 'pg', 'web'];

/** Workload profiles: a weight per section (non-negative). */
export const PROFILES = Object.freeze({
  balanced: Object.freeze({ label: 'Balanced', weights: Object.freeze({ cpu: 1, disk: 1, mem: 1, net: 1, pg: 1, web: 1 }) }),
  web: Object.freeze({ label: 'Web app', weights: Object.freeze({ cpu: 1, disk: 0.5, mem: 1, net: 2, pg: 0.5, web: 3 }) }),
  db: Object.freeze({ label: 'Database', weights: Object.freeze({ cpu: 1, disk: 3, mem: 2, net: 0.5, pg: 3, web: 0 }) }),
  compute: Object.freeze({ label: 'Compute', weights: Object.freeze({ cpu: 3, disk: 0.5, mem: 2, net: 0, pg: 0, web: 0 }) }),
});

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

/** Rows of one compared side that count for a score (see the header). */
function usedRows(comparison, otherIndex) {
  return (comparison.rows || []).filter((r) => r.other === otherIndex && r.cross_arch === false
    && isNum(r.base) && r.base > 0 && isNum(r.value) && r.value > 0);
}

function median(xs) {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

/** `{section: {score, n, weak, counts: {better, worse, not_clear}}}` for compared side `otherIndex`; score null when n == 0. */
export function sectionScores(comparison, otherIndex) {
  const out = {};
  for (const sec of SECTIONS) {
    const rows = usedRows(comparison, otherIndex).filter((r) => r.section === sec);
    const counts = { better: 0, worse: 0, not_clear: 0 };
    let sum = 0;
    for (const r of rows) {
      sum += Math.log(r.direction === 'lower' ? r.base / r.value : r.value / r.base);
      if (r.verdict in counts) counts[r.verdict]++;
    }
    out[sec] = { score: rows.length ? 100 * Math.exp(sum / rows.length) : null, n: rows.length, weak: rows.length < 3, counts };
  }
  return out;
}

/** `{section: median cv_value in % | null}` of compared side `otherIndex`, over the rows its section score uses. */
export function sideStability(comparison, otherIndex) {
  const rows = usedRows(comparison, otherIndex);
  const out = {};
  for (const sec of SECTIONS) out[sec] = median(rows.filter((r) => r.section === sec && isNum(r.cv_value)).map((r) => r.cv_value));
  return out;
}

/**
 * `{section: median cv_base in % | null}` of the baseline. A metric counts once (the baseline's CV does not
 * depend on the compared side) when at least one compared side uses its row for a score.
 */
export function baseStability(comparison) {
  const byId = new Map();
  for (let i = 0; i < (comparison.others || []).length; i++) {
    for (const r of usedRows(comparison, i)) if (!byId.has(r.id)) byId.set(r.id, r);
  }
  const rows = [...byId.values()];
  const out = {};
  for (const sec of SECTIONS) out[sec] = median(rows.filter((r) => r.section === sec && isNum(r.cv_base)).map((r) => r.cv_base));
  return out;
}

/**
 * `{section: median noise threshold in % | null}`: the `noise` of the rows the section scores use, pooled over
 * every compared side (a metric counts once per side). The stability chart draws it as the "within noise" band.
 */
export function medianNoise(comparison) {
  const rows = (comparison.others || []).flatMap((_, i) => usedRows(comparison, i));
  const out = {};
  for (const sec of SECTIONS) out[sec] = median(rows.filter((r) => r.section === sec && isNum(r.noise)).map((r) => r.noise));
  return out;
}

/**
 * Weighted geometric mean of the section scores that are not null (weights renormalised over them).
 * `{score, incomplete}`: `incomplete` lists sections with weight > 0 and no score; `score` is null when no
 * section with weight > 0 has one. A section missing from `weights` has weight 0.
 */
export function overallScore(scores, weights) {
  let sum = 0;
  let total = 0;
  const incomplete = [];
  for (const sec of SECTIONS) {
    const w = weights && isNum(weights[sec]) && weights[sec] > 0 ? weights[sec] : 0;
    if (!w) continue;
    const s = scores && scores[sec] ? scores[sec].score : null;
    if (!isNum(s)) { incomplete.push(sec); continue; }
    sum += w * Math.log(s / 100);
    total += w;
  }
  return { score: total > 0 ? 100 * Math.exp(sum / total) : null, incomplete };
}

/** `cpu:1,disk:3,...` to `{section: weight}` (every section, missing ones 0), or null when it is not valid. */
export function parseWeights(str) {
  if (typeof str !== 'string' || !str.trim()) return null;
  const out = Object.fromEntries(SECTIONS.map((s) => [s, 0]));
  const seen = new Set();
  for (const part of str.split(',')) {
    const m = /^\s*([a-z]+):(\d+(?:\.\d+)?)\s*$/.exec(part);
    if (!m || !SECTIONS.includes(m[1]) || seen.has(m[1])) return null;
    const w = Number(m[2]);
    if (!Number.isFinite(w)) return null;
    seen.add(m[1]);
    out[m[1]] = w;
  }
  return SECTIONS.some((s) => out[s] > 0) ? out : null;
}

/**
 * The six custom weights as typed (`{section: text}`), checked as the profile control allows them: a number from
 * 0 to 5 in steps of 0.5, at least one above 0. `{weights}` (every section), or `{error, section}` with `section`
 * the first offending one (null when the error is about all of them). parseWeights reads any link; this is the
 * stricter check of what the user types.
 */
export function checkWeights(texts) {
  const weights = {};
  for (const sec of SECTIONS) {
    const t = String((texts && texts[sec]) ?? '').trim();
    const v = Number(t);
    if (!t || !Number.isFinite(v)) return { section: sec, error: 'enter a number from 0 to 5' };
    if (v < 0) return { section: sec, error: 'a weight cannot be negative' };
    if (v > 5) return { section: sec, error: 'the largest weight is 5' };
    if (!Number.isInteger(v * 2)) return { section: sec, error: 'use steps of 0.5' };
    weights[sec] = v;
  }
  if (!SECTIONS.some((s) => weights[s] > 0)) return { section: null, error: 'at least one weight must be above 0' };
  return { weights };
}

/** `{section: weight}` to `cpu:1,disk:3,...`, sections in the fixed order; parseWeights reads it back. */
export function formatWeights(obj) {
  return SECTIONS.map((s) => s + ':' + (isNum(obj && obj[s]) && obj[s] > 0 ? obj[s] : 0)).join(',');
}

/** Weights of the URL state: `{profile, weights, invalid}`; an unknown profile or invalid custom weights fall back to balanced. */
export function weightsFor(profile, w) {
  if (profile === 'custom') {
    const parsed = parseWeights(w);
    if (parsed) return { profile: 'custom', weights: parsed, invalid: false };
    return { profile: 'balanced', weights: PROFILES.balanced.weights, invalid: true };
  }
  if (Object.hasOwn(PROFILES, profile)) return { profile, weights: PROFILES[profile].weights, invalid: false };
  return { profile: 'balanced', weights: PROFILES.balanced.weights, invalid: Boolean(profile) };
}

/**
 * Everything the blocks read, computed once: `{ weights, noise, base: {label, stability, overall}, sides: [{index,
 * label, sections, stability, overall}] }` (`noise` = medianNoise). The baseline is 100 by definition.
 */
export function scoresFor(comparison, weights) {
  return {
    weights,
    noise: medianNoise(comparison),
    base: { label: comparison.base.label, stability: baseStability(comparison), overall: { score: 100, incomplete: [] } },
    sides: (comparison.others || []).map((side, index) => {
      const sections = sectionScores(comparison, index);
      return { index, label: side.label, sections, stability: sideStability(comparison, index), overall: overallScore(sections, weights) };
    }),
  };
}

// ---- price ----

const PRICE_RE = /^\d+(?:\.\d+)?$/;
const tagOf = (run, key) => (run && run.tags && typeof run.tags[key] === 'string' ? run.tags[key].trim() : '');
const runName = (run) => run.label || run.run_id;

/**
 * Price of a side from the `price` and `price_unit` tags of its runs (`side.run_ids` looked up in `runs`):
 * `{price, unit}` (unit '' when there is none), or `{error: 'none' | 'invalid' | 'inconsistent', detail}`.
 * All runs must carry the same price and unit; `detail` names the runs that differ.
 */
export function sidePrice(side, runs) {
  const byId = new Map((runs || []).map((r) => [r.run_id, r]));
  const found = (side.run_ids || []).map((id) => byId.get(id)).filter(Boolean);
  const priced = found.filter((r) => tagOf(r, 'price') !== '');
  if (!priced.length) return { error: 'none', detail: '' };
  const bad = priced.filter((r) => !PRICE_RE.test(tagOf(r, 'price')) || !(Number(tagOf(r, 'price')) > 0) || !Number.isFinite(Number(tagOf(r, 'price'))));
  if (bad.length) return { error: 'invalid', detail: bad.map((r) => `${runName(r)}: ${tagOf(r, 'price')}`).join('; ') };
  const show = (r) => {
    const p = tagOf(r, 'price');
    const u = tagOf(r, 'price_unit');
    return `${runName(r)}: ${p === '' ? 'no price' : p}${u ? ' ' + u : ''}`;
  };
  const first = priced[0];
  const same = (r) => tagOf(r, 'price') !== '' && Number(tagOf(r, 'price')) === Number(tagOf(first, 'price')) && tagOf(r, 'price_unit') === tagOf(first, 'price_unit');
  if (!found.every(same)) return { error: 'inconsistent', detail: found.map(show).join('; ') };
  return { price: Number(tagOf(first, 'price')), unit: tagOf(first, 'price_unit') };
}

/**
 * Performance per price. `entries`: `{key, label, score, price, isBase}` per side, baseline included (its score
 * is 100), `price` = the result of sidePrice. Result: `{unit, show, usable, problems, indexFrom}`.
 * - `unit`: the unit shared by the most priced sides (the baseline's wins a tie, else the first listed).
 * - `usable`: entries priced in `unit` with a score, each with `perf` (score / price) and `index` (perf scaled
 *   so the baseline, or when it is not usable the cheapest side, is 100); `indexFrom` is that entry's key.
 * - `problems`: `{key, label, reason: 'none' | 'invalid' | 'inconsistent' | 'different unit' | 'no score', detail}`.
 * - `show`: at least 2 usable sides.
 */
export function pricePerformance(entries) {
  const priced = (entries || []).filter((e) => e.price && isNum(e.price.price));
  const tally = new Map();
  for (const e of priced) tally.set(e.price.unit, (tally.get(e.price.unit) || 0) + 1);
  let unit = '';
  let best = 0;
  for (const [u, n] of tally) if (n > best) { best = n; unit = u; }
  const base = priced.find((e) => e.isBase);
  if (base && tally.get(base.price.unit) === best) unit = base.price.unit;
  const problems = [];
  const usable = [];
  for (const e of entries || []) {
    const p = e.price;
    if (!p || p.error) problems.push({ key: e.key, label: e.label, reason: p && p.error ? p.error : 'none', detail: p && p.detail ? p.detail : '' });
    else if (p.unit !== unit) problems.push({ key: e.key, label: e.label, reason: 'different unit', detail: p.unit || 'no unit' });
    else if (!isNum(e.score) || e.score <= 0) problems.push({ key: e.key, label: e.label, reason: 'no score', detail: '' });
    else usable.push({ ...e, price: p.price, unit: p.unit, perf: e.score / p.price });
  }
  const ref = usable.find((e) => e.isBase) || usable.reduce((a, b) => (a && a.price <= b.price ? a : b), null);
  for (const e of usable) e.index = ref ? 100 * e.perf / ref.perf : null;
  return { unit, show: usable.length >= 2, usable, problems, indexFrom: ref ? ref.key : null };
}

/**
 * Keys of the Pareto frontier of `points` (`{key, price, score}`): the points no other point beats, i.e. none is
 * at most as expensive and at least as good, and strictly better on one of the two.
 */
export function paretoFrontier(points) {
  const out = new Set();
  for (const e of points || []) {
    const beaten = points.some((o) => o !== e && o.price <= e.price && o.score >= e.score && (o.price < e.price || o.score > e.score));
    if (!beaten) out.add(e.key);
  }
  return out;
}

// ---- scaling ----

/**
 * vCPU of a side: the nproc of its runs (`side.run_ids` in `runs`); when none is found, of its group
 * (`group:NAME[@KEY=VALUE]` in `groups`). `{vcpu}`, or `{error: 'mixed' | 'none', detail}`.
 */
export function sideVcpu(side, runs, groups) {
  const byId = new Map((runs || []).map((r) => [r.run_id, r]));
  let counts = (side.run_ids || []).map((id) => byId.get(id)).filter((r) => r && isNum(r.nproc)).map((r) => r.nproc);
  if (!counts.length && typeof side.reference === 'string' && side.reference.startsWith('group:')) {
    const name = side.reference.slice(6).split('@')[0];
    const g = (groups || []).find((x) => x.group === name);
    counts = g ? (g.nproc || []).filter(isNum) : [];
  }
  const distinct = [...new Set(counts)].sort((a, b) => a - b);
  if (!distinct.length) return { error: 'none', detail: 'no vCPU count known' };
  if (distinct.length > 1) return { error: 'mixed', detail: distinct.join(', ') + ' vCPU' };
  return { vcpu: distinct[0] };
}

/**
 * Scaling efficiency of `point` against `ref`, the smallest point of the same arch (both `{vcpu, score}`):
 * `expected` = what linear scaling from `ref` gives (`ref.score x point.vcpu / ref.vcpu`), `percent` = the score
 * against it in % (100 for `ref` itself).
 */
export function scalingEfficiency(point, ref) {
  const expected = ref.score * point.vcpu / ref.vcpu;
  return { expected, percent: 100 * point.score / expected };
}
