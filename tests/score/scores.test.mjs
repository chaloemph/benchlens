// Section scores, stability and overall scores of web/js/score.js against tools/score_check.py.

import test from 'node:test';
import assert from 'node:assert/strict';
import { sectionScores, sideStability, baseStability, overallScore, PROFILES, SECTIONS, scoresFor } from '../../web/js/score.js';
import { example, clone, close, checkAgainstPython } from './_helpers.mjs';

const real = () => example('compare');

test('real example comparison: every score equals score_check.py', () => {
  checkAgainstPython(real(), 'example');
});

test('the example has data to check: a side with scores and a side with none', () => {
  const c = real();
  const s0 = sectionScores(c, 0);
  assert.ok(SECTIONS.some((s) => s0[s].score !== null));
  const s1 = sectionScores(c, 1);
  assert.ok(SECTIONS.every((s) => s1[s].n === 0 && s1[s].score === null), 'a side without rows has no score, not 0');
  assert.equal(overallScore(s1, PROFILES.balanced.weights).score, null);
  assert.deepEqual(overallScore(s1, PROFILES.balanced.weights).incomplete, SECTIONS);
});

// A comparison with the three real rows spread over sections and directions, for the edge cases.
function spread() {
  const c = real();
  const rows = c.rows.filter((r) => r.other === 0);
  assert.ok(rows.length >= 3);
  const more = [];
  const secs = ['cpu', 'disk', 'mem', 'net', 'pg', 'web'];
  secs.forEach((sec, i) => {
    for (let k = 0; k < i + 1; k++) {
      const r = clone(rows[(i + k) % rows.length]);
      r.section = sec;
      r.id = `${sec}.row${k}`;
      r.value = r.value * (1 + 0.1 * (k + 1));
      more.push(r);
    }
  });
  c.rows = more;
  return c;
}

test('spread comparison (n from 1 to 6 per section) equals score_check.py', () => {
  const c = spread();
  checkAgainstPython(c, 'spread');
  const s = sectionScores(c, 0);
  assert.deepEqual(SECTIONS.map((x) => s[x].n), [1, 2, 3, 4, 5, 6]);
  assert.deepEqual(SECTIONS.map((x) => s[x].weak), [true, true, false, false, false, false]);
});

test('null and non-positive values are skipped', () => {
  const c = spread();
  c.rows[0].value = null;
  c.rows[1].base = null;
  c.rows[2].value = 0;
  c.rows[3].base = 0;
  c.rows[4].base = -1;
  checkAgainstPython(c, 'nulls');
  const s = sectionScores(c, 0);
  assert.equal(s.cpu.score, null, 'cpu had one row and it is null');
  assert.equal(s.cpu.n, 0);
  assert.equal(s.disk.n, 0, 'both disk rows skipped (null base, zero value)');
});

test('cross_arch rows are left out of scores and counts', () => {
  const c = spread();
  for (const r of c.rows.filter((x) => x.section === 'mem')) r.cross_arch = true;
  checkAgainstPython(c, 'cross');
  const s = sectionScores(c, 0);
  assert.equal(s.mem.n, 0);
  assert.equal(s.mem.score, null);
});

test('direction lower inverts the ratio; direction higher does not', () => {
  const c = spread();
  const row = c.rows.find((r) => r.section === 'cpu');
  row.direction = 'higher'; row.base = 100; row.value = 200;
  assert.ok(Math.abs(sectionScores(c, 0).cpu.score - 200) < 1e-9);
  row.direction = 'lower';
  assert.ok(Math.abs(sectionScores(c, 0).cpu.score - 50) < 1e-9);
  for (const r of c.rows) if (r.section === 'net') r.direction = 'lower';
  checkAgainstPython(c, 'lower');
});

test('score is the geometric mean: one 5x outlier does not dominate', () => {
  const c = spread();
  c.rows = c.rows.filter((r) => r.section === 'cpu');
  c.rows.forEach((r) => { r.direction = 'higher'; r.base = 100; });
  c.rows[0].value = 500;
  for (const r of c.rows.slice(1)) r.value = 100;
  const n = c.rows.length;
  assert.ok(Math.abs(sectionScores(c, 0).cpu.score - 100 * Math.pow(5, 1 / n)) < 1e-9);
});

test('rows of other sides are not mixed in', () => {
  const c = spread();
  const extra = clone(c.rows.filter((r) => r.section === 'cpu'));
  extra.forEach((r) => { r.other = 1; r.value = r.base; });
  c.rows.push(...extra);
  checkAgainstPython(c, 'two sides');
  assert.ok(Math.abs(sectionScores(c, 1).cpu.score - 100) < 1e-9);
});

test('weight 0 ignores a section, also when it has no data', () => {
  const c = spread();
  const s = sectionScores(c, 0);
  const w = { cpu: 1, disk: 0, mem: 0, net: 0, pg: 0, web: 0 };
  close(overallScore(s, w).score, s.cpu.score, 'only cpu');
  const noWeb = { ...s, web: { ...s.web, score: null, n: 0 } };
  const o = overallScore(noWeb, { ...PROFILES.db.weights });
  assert.deepEqual(o.incomplete, [], 'db gives web weight 0: nothing is missing');
  const o2 = overallScore(noWeb, PROFILES.web.weights);
  assert.deepEqual(o2.incomplete, ['web']);
  assert.ok(o2.score > 0, 'the other sections still give a score');
});

test('all weights 0, or a profile that needs only missing sections: no score', () => {
  const c = spread();
  const s = sectionScores(c, 0);
  assert.equal(overallScore(s, { cpu: 0, disk: 0, mem: 0, net: 0, pg: 0, web: 0 }).score, null);
  const none = Object.fromEntries(SECTIONS.map((x) => [x, { score: null, n: 0, weak: true, counts: {} }]));
  assert.equal(overallScore(none, PROFILES.compute.weights).score, null);
  assert.deepEqual(overallScore(none, PROFILES.compute.weights).incomplete, ['cpu', 'disk', 'mem']);
});

test('weights are renormalised: scaling every weight changes nothing', () => {
  const s = sectionScores(spread(), 0);
  close(overallScore(s, PROFILES.web.weights).score,
    overallScore(s, Object.fromEntries(SECTIONS.map((x) => [x, PROFILES.web.weights[x] * 7]))).score, 'scaled');
});

test('stability: median CV over used rows; all-null CV gives null; measured once', () => {
  const c = spread();
  checkAgainstPython(c, 'stability');
  for (const r of c.rows) { r.cv_value = null; r.cv_base = null; }
  const st = sideStability(c, 0);
  assert.ok(SECTIONS.every((x) => st[x] === null));
  assert.ok(SECTIONS.every((x) => baseStability(c)[x] === null));
  checkAgainstPython(c, 'all-null CV');
});

test('stability: some CV null are skipped, medians of even counts average the middle', () => {
  const c = spread();
  const netRows = c.rows.filter((r) => r.section === 'net');
  netRows.forEach((r, i) => { r.cv_value = [1, 3, 8, 20][i] ?? 2; });
  assert.ok(netRows.length >= 4);
  checkAgainstPython(c, 'median');
});

test('baseStability counts a metric once even when several sides use it', () => {
  const c = spread();
  const dup = clone(c.rows);
  dup.forEach((r) => { r.other = 1; });
  c.others = [...c.others, c.others[0]];
  const before = baseStability(c);
  c.rows.push(...dup);
  const after = baseStability(c);
  for (const sec of SECTIONS) close(after[sec], before[sec], 'base stability ' + sec);
  checkAgainstPython(c, 'two sides base');
});

test('scoresFor: baseline 100, one entry per compared side, profile applied', () => {
  const c = spread();
  const w = PROFILES.db.weights;
  const sc = scoresFor(c, w);
  assert.equal(sc.base.overall.score, 100);
  assert.equal(sc.sides.length, c.others.length);
  assert.equal(sc.sides[0].label, c.others[0].label);
  close(sc.sides[0].overall.score, overallScore(sectionScores(c, 0), w).score, 'overall');
});
