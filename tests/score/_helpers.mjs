// Shared by the score tests: the real API examples (docs/api-examples), and the expected numbers, which always
// come from tools/score_check.py run on the same JSON, never typed by hand.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { sectionScores, sideStability, baseStability, medianNoise, overallScore, PROFILES, SECTIONS, weightsFor } from '../../web/js/score.js';

export const ROOT = path.resolve(import.meta.dirname, '../..');
const EXAMPLES = path.join(ROOT, 'docs/api-examples');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'benchlens-score-'));
process.on('exit', () => fs.rmSync(tmp, { recursive: true, force: true }));

/** An example file; long lists are cut there with a "... N more" string, which is dropped here. */
export function example(name) {
  const data = JSON.parse(fs.readFileSync(path.join(EXAMPLES, name + '.json'), 'utf8'));
  const clean = (x) => (Array.isArray(x) ? x.filter((v) => typeof v !== 'string').map(clean) : x);
  if (Array.isArray(data)) return clean(data);
  return Object.fromEntries(Object.entries(data).map(([k, v]) => [k, clean(v)]));
}

export const clone = (x) => structuredClone(x);

let n = 0;
/** What tools/score_check.py recomputes for a comparison (a JS object), optionally with custom weights. */
export function expected(comparison, weights = null) {
  const file = path.join(tmp, `cmp-${n++}.json`);
  fs.writeFileSync(file, JSON.stringify(comparison));
  const args = [path.join(ROOT, 'tools/score_check.py'), '--json', '--compare', file];
  if (weights) args.push('--weights', weights);
  return JSON.parse(execFileSync('python3', args, { encoding: 'utf8', maxBuffer: 1 << 28 }));
}

/** Equal to 1e-9 (relative above 1), null equal to null. */
export function close(actual, want, what) {
  if (want === null || actual === null) { assert.equal(actual, want, what); return; }
  assert.ok(Math.abs(actual - want) <= 1e-9 * Math.max(1, Math.abs(want)), `${what}: ${actual} vs ${want}`);
}

export function sameSection(actual, want, what) {
  close(actual.score, want.score, what + ' score');
  assert.equal(actual.n, want.n, what + ' n');
  assert.equal(actual.weak, want.weak, what + ' weak');
  assert.deepEqual(actual.counts, want.counts, what + ' counts');
}

const CUSTOM = 'cpu:1,disk:3,mem:0,net:2.5,pg:0.5,web:4';

/** Every number of score.js for `comparison` equals the recomputation of score_check.py. */
export function checkAgainstPython(comparison, label) {
  const want = expected(comparison, CUSTOM);
  const profiles = { ...Object.fromEntries(Object.entries(PROFILES).map(([k, v]) => [k, v.weights])), custom: weightsFor('custom', CUSTOM).weights };
  const base = baseStability(comparison);
  for (const sec of SECTIONS) close(base[sec], want.base_stability[sec], `${label} base stability ${sec}`);
  const noise = medianNoise(comparison);
  for (const sec of SECTIONS) close(noise[sec], want.median_noise[sec], `${label} median noise ${sec}`);
  assert.equal(comparison.others.length, want.sides.length);
  comparison.others.forEach((side, i) => {
    const w = want.sides[i];
    const sections = sectionScores(comparison, i);
    const stab = sideStability(comparison, i);
    for (const sec of SECTIONS) {
      sameSection(sections[sec], w.sections[sec], `${label} ${side.label} ${sec}`);
      close(stab[sec], w.stability[sec], `${label} ${side.label} stability ${sec}`);
    }
    for (const [name, weights] of Object.entries(profiles)) {
      const o = overallScore(sections, weights);
      close(o.score, w.overall[name].score, `${label} ${side.label} overall ${name}`);
      assert.deepEqual(o.incomplete, w.overall[name].incomplete, `${label} ${side.label} incomplete ${name}`);
    }
  });
}

