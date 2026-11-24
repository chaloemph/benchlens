import test from 'node:test';
import assert from 'node:assert/strict';
import { parseWeights, formatWeights, weightsFor, PROFILES, SECTIONS } from '../../web/js/score.js';

test('profiles: the built-in weights', () => {
  assert.deepEqual(Object.keys(PROFILES), ['balanced', 'web', 'db', 'compute']);
  assert.deepEqual(PROFILES.balanced.weights, { cpu: 1, disk: 1, mem: 1, net: 1, pg: 1, web: 1 });
  assert.deepEqual(PROFILES.web.weights, { cpu: 1, disk: 0.5, mem: 1, net: 2, pg: 0.5, web: 3 });
  assert.deepEqual(PROFILES.db.weights, { cpu: 1, disk: 3, mem: 2, net: 0.5, pg: 3, web: 0 });
  assert.deepEqual(PROFILES.compute.weights, { cpu: 3, disk: 0.5, mem: 2, net: 0, pg: 0, web: 0 });
  for (const p of Object.values(PROFILES)) assert.deepEqual(Object.keys(p.weights), SECTIONS);
});

test('formatWeights writes the fixed section order and parseWeights reads it back', () => {
  for (const p of Object.values(PROFILES)) assert.deepEqual(parseWeights(formatWeights(p.weights)), p.weights);
  assert.equal(formatWeights(PROFILES.web.weights), 'cpu:1,disk:0.5,mem:1,net:2,pg:0.5,web:3');
  assert.equal(formatWeights({ web: 2, cpu: 1 }), 'cpu:1,disk:0,mem:0,net:0,pg:0,web:2');
});

test('parseWeights: partial lists give 0 for the rest, spaces are fine', () => {
  assert.deepEqual(parseWeights('cpu:1, disk:3'), { cpu: 1, disk: 3, mem: 0, net: 0, pg: 0, web: 0 });
});

test('parseWeights: invalid input gives null', () => {
  for (const bad of ['', '   ', null, undefined, 42, 'cpu', 'cpu:', 'cpu:-1', 'cpu:abc', 'cpu:1;disk:2', 'gpu:1', 'cpu:1,cpu:2',
    'cpu:0,disk:0', 'cpu:1e3', 'cpu:+1', 'cpu:.5', 'cpu:1,', 'CPU:1', 'cpu:Infinity', 'cpu:NaN']) {
    assert.equal(parseWeights(bad), null, JSON.stringify(bad));
  }
});

test('weightsFor: profile and w of the URL state', () => {
  assert.deepEqual(weightsFor('db', ''), { profile: 'db', weights: PROFILES.db.weights, invalid: false });
  assert.deepEqual(weightsFor('', ''), { profile: 'balanced', weights: PROFILES.balanced.weights, invalid: false });
  assert.equal(weightsFor('custom', 'cpu:2,web:1').weights.cpu, 2);
  assert.equal(weightsFor('custom', 'cpu:2,web:1').profile, 'custom');
  const bad = weightsFor('custom', 'cpu:-1');
  assert.deepEqual([bad.profile, bad.invalid], ['balanced', true]);
  assert.deepEqual([weightsFor('nope', '').profile, weightsFor('nope', '').invalid], ['balanced', true]);
  assert.equal(weightsFor('constructor', '').profile, 'balanced', 'inherited property names are not profiles');
});
