// The same comparison on the whole fixtures: docs/api-examples cut long lists, so this one runs the real
// `benchlens compare --json` on tests/fixtures (the sides of docs/api-examples/compare.json) when a built
// binary is available (BENCHLENS_BIN, target/release or target/debug) and is skipped, loudly, when not. In CI
// (CI set, as GitHub Actions does) or when BENCHLENS_BIN names a missing file it fails instead of skipping.

import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { sectionScores, SECTIONS } from '../../web/js/score.js';
import { ROOT, example, checkAgainstPython } from './_helpers.mjs';

const named = process.env.BENCHLENS_BIN;
const candidates = named ? [named] : [path.join(ROOT, 'target/release/benchlens'), path.join(ROOT, 'target/debug/benchlens')];
const bin = candidates.find((p) => fs.existsSync(p));
const mustRun = Boolean(named || process.env.CI);

test('whole fixtures: every score equals score_check.py', { skip: bin || mustRun ? false : 'no benchlens binary (cargo build --release -p benchlens, or set BENCHLENS_BIN)' }, () => {
  assert.ok(bin, `no benchlens binary at ${candidates.join(' or ')}: build it first (cargo build -p benchlens)`);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'benchlens-score-full-'));
  try {
    const db = path.join(dir, 'full.db');
    const run = (...args) => execFileSync(bin, ['--db', db, ...args], { encoding: 'utf8', cwd: ROOT, maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'pipe'] });
    run('import', '--registry', 'examples/poc-2026-11/registry.tsv', 'tests/fixtures/v0.2.1', 'tests/fixtures/v0.2.3');
    const ex = example('compare');
    const sides = [ex.base.reference, ...ex.others.map((o) => o.reference)];
    const c = JSON.parse(run('compare', ...sides, '--json'));
    assert.ok(c.rows.length > 50, 'a real comparison has many rows');
    checkAgainstPython(c, 'fixtures');
    // the real data has several sections, and sysbench across architectures is not counted
    const s = sectionScores(c, 0);
    assert.ok(SECTIONS.filter((x) => s[x].score !== null).length >= 4);
    assert.ok(c.rows.some((r) => r.cross_arch), 'the fixtures have cross-architecture rows');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
