// Helpers the Summary blocks use and that moved into score.js at the Wave 1 review: medianNoise (stability
// band), checkWeights (custom weights as typed), paretoFrontier (price / performance), scalingEfficiency
// (scaling table). medianNoise is also recomputed by tools/score_check.py (see checkAgainstPython).

import test from 'node:test';
import assert from 'node:assert/strict';
import { medianNoise, checkWeights, paretoFrontier, scalingEfficiency, scoresFor, parseWeights, formatWeights, SECTIONS, PROFILES } from '../../web/js/score.js';
import { example, clone, close, checkAgainstPython } from './_helpers.mjs';

const cmp = example('compare');

test('medianNoise: equals score_check.py on the example; scoresFor carries it', () => {
  checkAgainstPython(cmp, 'example');
  const noise = medianNoise(cmp);
  assert.deepEqual(Object.keys(noise), SECTIONS);
  assert.deepEqual(scoresFor(cmp, PROFILES.balanced.weights).noise, noise);
});

test('medianNoise: only rows a score uses (cross_arch, null and non-positive values left out); no rows gives null', () => {
  const c = clone(cmp);
  const row = c.rows.find((r) => r.cross_arch === false && r.base > 0 && r.value > 0);
  for (const r of c.rows) if (r.section === row.section && r !== row) r.cross_arch = true;
  row.noise = 7.25;
  assert.equal(medianNoise(c)[row.section], 7.25);
  row.value = null;
  assert.equal(medianNoise(c)[row.section], null);
  checkAgainstPython(c, 'one row');
});

test('medianNoise: rows of every compared side are pooled; an even count averages the middle two', () => {
  const c = clone(cmp);
  c.others = [c.others[0], c.others[0]];
  const base = { id: 'x', name: 'x', section: 'net', unit: 'MB/s', direction: 'higher', base: 1, value: 1, cv_base: null, cv_value: null, n_base: 1, n_value: 1, diff: 0, verdict: 'not_clear', cross_arch: false };
  c.rows = [{ ...base, other: 0, noise: 2 }, { ...base, other: 0, noise: 4 }, { ...base, other: 1, noise: 10 }, { ...base, other: 1, noise: 30 }];
  assert.equal(medianNoise(c).net, 7);
  assert.equal(medianNoise(c).cpu, null);
  checkAgainstPython(c, 'pooled');
});

const texts = (over) => ({ cpu: '1', disk: '1', mem: '1', net: '1', pg: '1', web: '1', ...over });

test('checkWeights: 0 to 5 in steps of 0.5, at least one above 0', () => {
  assert.deepEqual(checkWeights(texts({ disk: '0', net: '2.5', web: ' 5 ' })), { weights: { cpu: 1, disk: 0, mem: 1, net: 2.5, pg: 1, web: 5 } });
  assert.deepEqual(checkWeights(texts({ mem: '-1' })), { section: 'mem', error: 'a weight cannot be negative' });
  assert.deepEqual(checkWeights(texts({ pg: '6' })), { section: 'pg', error: 'the largest weight is 5' });
  assert.deepEqual(checkWeights(texts({ cpu: '0.3' })), { section: 'cpu', error: 'use steps of 0.5' });
  for (const bad of ['', ' ', 'abc', 'Infinity']) assert.equal(checkWeights(texts({ net: bad })).section, 'net', JSON.stringify(bad));
  assert.deepEqual(checkWeights(Object.fromEntries(SECTIONS.map((s) => [s, '0']))), { section: null, error: 'at least one weight must be above 0' });
  assert.equal(checkWeights(null).section, 'cpu');
});

test('checkWeights: whatever it accepts, formatWeights writes and parseWeights reads back', () => {
  for (const t of [texts({}), texts({ cpu: '3', disk: '0', net: '0.5' }), texts({ cpu: '0', disk: '0', mem: '0', net: '0', pg: '0', web: '0.5' })]) {
    const { weights } = checkWeights(t);
    assert.deepEqual(parseWeights(formatWeights(weights)), weights);
  }
});

test('paretoFrontier: points beaten on price and score are left out; ties on both stay', () => {
  const pts = [
    { key: 'a', price: 100, score: 100 },
    { key: 'b', price: 150, score: 140 },
    { key: 'c', price: 150, score: 120 }, // b is as expensive and better
    { key: 'd', price: 200, score: 130 }, // b is cheaper and better
    { key: 'e', price: 90, score: 60 },
    { key: 'f', price: 90, score: 60 }, // the same as e: neither beats the other
  ];
  assert.deepEqual([...paretoFrontier(pts)].sort(), ['a', 'b', 'e', 'f']);
  assert.deepEqual([...paretoFrontier([])], []);
  assert.deepEqual([...paretoFrontier([pts[0]])], ['a']);
});

test('scalingEfficiency: score against linear scaling from the smallest point', () => {
  const ref = { vcpu: 2, score: 100 };
  assert.deepEqual(scalingEfficiency(ref, ref), { expected: 100, percent: 100 });
  const e = scalingEfficiency({ vcpu: 16, score: 400 }, ref);
  close(e.expected, 800, 'expected');
  close(e.percent, 50, 'percent');
  close(scalingEfficiency({ vcpu: 8, score: 90 }, { vcpu: 4, score: 30 }).percent, 150, 'super-linear');
});
