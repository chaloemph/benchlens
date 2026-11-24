// Prices and vCPU of a side. The runs are the real runs of docs/api-examples/runs.json with example prices
// (user input, not measurements) put in their tags.

import test from 'node:test';
import assert from 'node:assert/strict';
import { sidePrice, pricePerformance, sideVcpu, overallScore, sectionScores, PROFILES } from '../../web/js/score.js';
import { example, clone, expected, close } from './_helpers.mjs';

const realRuns = example('runs');
const comparison = example('compare');

function runsWith(specs) {
  // specs: [id, label, tags] cloned from a real run
  return specs.map(([id, label, tags]) => ({ ...clone(realRuns[0]), run_id: id, label, tags: { ...tags } }));
}
const side = (ids, extra = {}) => ({ reference: 'group:t', label: 't', run_ids: ids, archs: ['x86_64'], ...extra });

test('sidePrice: no price tag is "none"', () => {
  const runs = runsWith([['a', 'ra', {}], ['b', 'rb', { note: 'x' }]]);
  assert.deepEqual(sidePrice(side(['a', 'b']), runs), { error: 'none', detail: '' });
  assert.equal(sidePrice(side([]), runs).error, 'none');
  assert.equal(sidePrice(side(['unknown-run']), runs).error, 'none');
});

test('sidePrice: a usable price and unit', () => {
  const runs = runsWith([['a', 'ra', { price: '4500', price_unit: 'THB/month' }], ['b', 'rb', { price: '4500', price_unit: 'THB/month' }]]);
  assert.deepEqual(sidePrice(side(['a', 'b']), runs), { price: 4500, unit: 'THB/month' });
  const dec = runsWith([['a', 'ra', { price: '0.096', price_unit: 'USD/hour' }]]);
  assert.deepEqual(sidePrice(side(['a']), dec), { price: 0.096, unit: 'USD/hour' });
});

test('sidePrice: no unit is the empty unit', () => {
  const runs = runsWith([['a', 'ra', { price: '12' }]]);
  assert.deepEqual(sidePrice(side(['a']), runs), { price: 12, unit: '' });
});

test('sidePrice: invalid prices are reported with the run', () => {
  for (const bad of ['abc', '0', '-5', '1,5', '1 500', '1e3', '+4', '.5', '4500.', '€5', '1.2.3', 'NaN', 'Infinity', '0.0']) {
    const runs = runsWith([['a', 'ra', { price: bad }]]);
    const p = sidePrice(side(['a']), runs);
    assert.equal(p.error, 'invalid', JSON.stringify(bad));
    assert.ok(p.detail.includes('ra'), p.detail);
  }
  const mixed = runsWith([['a', 'ra', { price: '10' }], ['b', 'rb', { price: 'ten' }]]);
  assert.equal(sidePrice(side(['a', 'b']), mixed).error, 'invalid');
});

test('sidePrice: surrounding spaces are allowed', () => {
  assert.deepEqual(sidePrice(side(['a']), runsWith([['a', 'ra', { price: ' 15 ', price_unit: ' USD ' }]])), { price: 15, unit: 'USD' });
});

test('sidePrice: runs that differ are inconsistent and named', () => {
  const diffPrice = runsWith([['a', 'run-a', { price: '10' }], ['b', 'run-b', { price: '11' }]]);
  const p = sidePrice(side(['a', 'b']), diffPrice);
  assert.equal(p.error, 'inconsistent');
  assert.ok(p.detail.includes('run-a') && p.detail.includes('run-b'));
  const diffUnit = runsWith([['a', 'run-a', { price: '10', price_unit: 'USD' }], ['b', 'run-b', { price: '10', price_unit: 'EUR' }]]);
  assert.equal(sidePrice(side(['a', 'b']), diffUnit).error, 'inconsistent');
  const oneMissing = runsWith([['a', 'run-a', { price: '10' }], ['b', 'run-b', {}]]);
  const q = sidePrice(side(['a', 'b']), oneMissing);
  assert.equal(q.error, 'inconsistent');
  assert.ok(q.detail.includes('run-b: no price'));
  const unitOnlyOne = runsWith([['a', 'run-a', { price: '10', price_unit: 'USD' }], ['b', 'run-b', { price: '10' }]]);
  assert.equal(sidePrice(side(['a', 'b']), unitOnlyOne).error, 'inconsistent');
});

test('sidePrice: 10 and 10.0 are the same price', () => {
  const runs = runsWith([['a', 'ra', { price: '10' }], ['b', 'rb', { price: '10.0' }]]);
  assert.equal(sidePrice(side(['a', 'b']), runs).price, 10);
});

test('sidePrice: tags are untrusted text, passed through as data', () => {
  const runs = runsWith([['a', '<img src=x onerror=1>', { price: '<b>1</b>' }]]);
  const p = sidePrice(side(['a']), runs);
  assert.equal(p.error, 'invalid');
  assert.equal(typeof p.detail, 'string');
});

// ---- pricePerformance ----

const E = (key, score, price, isBase = false) => ({ key, label: key, score, price, isBase });
const P = (price, unit = '') => ({ price, unit });

test('pricePerformance: baseline = 100 when it has a usable price; scores come from score_check', () => {
  const want = expected(comparison).sides[0].overall.balanced.score;
  assert.ok(want > 0);
  const out = pricePerformance([E('base', 100, P(1000, 'USD/h'), true), E('s0', want, P(500, 'USD/h')), E('s1', 150, P(2000, 'USD/h'))]);
  assert.equal(out.show, true);
  assert.equal(out.unit, 'USD/h');
  assert.equal(out.indexFrom, 'base');
  const by = Object.fromEntries(out.usable.map((e) => [e.key, e]));
  close(by.base.index, 100, 'base');
  close(by.s0.perf, want / 500, 's0 perf');
  close(by.s0.index, 100 * (want / 500) / (100 / 1000), 's0 index');
  close(by.s1.index, 100 * (150 / 2000) / (100 / 1000), 's1 index');
});

test('pricePerformance: cheapest priced side = 100 when the baseline has no usable price', () => {
  const out = pricePerformance([E('base', 100, { error: 'none', detail: '' }, true), E('a', 120, P(300)), E('b', 90, P(100))]);
  assert.equal(out.show, true);
  assert.equal(out.indexFrom, 'b');
  const by = Object.fromEntries(out.usable.map((e) => [e.key, e]));
  close(by.b.index, 100, 'cheapest');
  close(by.a.index, 100 * (120 / 300) / (90 / 100), 'a');
  assert.deepEqual(out.problems.map((p) => [p.key, p.reason]), [['base', 'none']]);
});

test('pricePerformance: needs two usable sides', () => {
  assert.equal(pricePerformance([E('base', 100, P(10), true), E('a', 90, { error: 'none', detail: '' })]).show, false);
  assert.equal(pricePerformance([E('base', 100, P(10), true)]).show, false);
  assert.equal(pricePerformance([]).show, false);
  assert.equal(pricePerformance([E('base', 100, { error: 'invalid', detail: 'x' }, true), E('a', 90, { error: 'inconsistent', detail: 'y' })]).show, false);
});

test('pricePerformance: the unit shared by most sides; the baseline unit wins a tie; others are "different unit"', () => {
  const most = pricePerformance([E('base', 100, P(10, 'USD'), true), E('a', 90, P(5, 'EUR')), E('b', 80, P(4, 'EUR'))]);
  assert.equal(most.unit, 'EUR');
  assert.deepEqual(most.problems.map((p) => [p.key, p.reason, p.detail]), [['base', 'different unit', 'USD']]);
  assert.equal(most.show, true);
  const tie = pricePerformance([E('a', 90, P(5, 'EUR')), E('base', 100, P(10, 'USD'), true), E('b', 80, P(4, 'EUR')), E('c', 70, P(3, 'USD'))]);
  assert.equal(tie.unit, 'USD', 'tie: the baseline unit');
  assert.deepEqual(tie.problems.map((p) => p.key).sort(), ['a', 'b']);
  const tieNoBase = pricePerformance([E('a', 90, P(5, 'EUR')), E('b', 80, P(4, 'USD'))]);
  assert.equal(tieNoBase.unit, 'EUR', 'tie without a priced baseline: first listed');
  assert.equal(tieNoBase.show, false);
  const none = pricePerformance([E('a', 90, P(5, '')), E('b', 80, P(4, ''))]);
  assert.equal(none.show, true, 'both without unit count as the same unit');
});

test('pricePerformance: a side without a score is a problem, not a point', () => {
  const out = pricePerformance([E('base', 100, P(10), true), E('a', null, P(5)), E('b', 80, P(4))]);
  assert.deepEqual(out.problems.map((p) => [p.key, p.reason]), [['a', 'no score']]);
  assert.equal(out.usable.length, 2);
});

test('pricePerformance with real overall scores of the example comparison', () => {
  const c = example('compare');
  const want = expected(c).sides[0].overall.web.score;
  const got = overallScore(sectionScores(c, 0), PROFILES.web.weights).score;
  close(got, want, 'overall');
  const out = pricePerformance([E('base', 100, P(4500, 'THB/month'), true), E('arm', got, P(3600, 'THB/month'))]);
  close(out.usable[1].index, 100 * (want / 3600) / (100 / 4500), 'index');
});

// ---- vCPU ----

const groups = example('groups');

test('sideVcpu: from the runs of the side', () => {
  const runs = [{ run_id: 'a', nproc: 16 }, { run_id: 'b', nproc: 16 }];
  assert.deepEqual(sideVcpu(side(['a', 'b']), runs, groups), { vcpu: 16 });
});

test('sideVcpu: real runs of the example', () => {
  const r = realRuns[0];
  assert.deepEqual(sideVcpu(side([r.run_id]), realRuns, groups), { vcpu: r.nproc });
});

test('sideVcpu: mixed vCPU counts are an error naming them', () => {
  const runs = [{ run_id: 'a', nproc: 2 }, { run_id: 'b', nproc: 16 }];
  const v = sideVcpu(side(['a', 'b']), runs, groups);
  assert.equal(v.error, 'mixed');
  assert.ok(v.detail.includes('2') && v.detail.includes('16'));
});

test('sideVcpu: falls back to the group, also for a split group; mixed or unknown groups fail', () => {
  const g = groups.find((x) => x.nproc.length === 1);
  assert.deepEqual(sideVcpu({ reference: 'group:' + g.group, run_ids: [] }, [], groups), { vcpu: g.nproc[0] });
  assert.deepEqual(sideVcpu({ reference: `group:${g.group}@round=1`, run_ids: ['gone'] }, [], groups), { vcpu: g.nproc[0] });
  const mixed = [{ group: 'm', nproc: [2, 16] }];
  assert.equal(sideVcpu({ reference: 'group:m', run_ids: [] }, [], mixed).error, 'mixed');
  assert.equal(sideVcpu({ reference: 'group:nope', run_ids: [] }, [], groups).error, 'none');
  assert.equal(sideVcpu({ reference: 'abcdef', run_ids: ['zz'] }, [], groups).error, 'none');
  assert.equal(sideVcpu({ reference: 'group:' + g.group, run_ids: ['a'] }, [{ run_id: 'a', nproc: null }], groups).vcpu, g.nproc[0], 'null nproc falls back');
});
