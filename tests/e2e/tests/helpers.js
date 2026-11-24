// Shared helpers: API access, shape checks, the comparisons under test (picked from the data at run time),
// a page fixture that fails on console errors, and the UI's documented number formats.

import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from '@playwright/test';

export { expect };

/** Fixture folders imported by scripts/e2e.sh (tests/fixtures/<version>/<folder>). */
export function fixtureFolders() {
  const root = process.env.BENCHLENS_FIXTURES || path.resolve(import.meta.dirname, '../../fixtures');
  return fs.readdirSync(root, { withFileTypes: true }).filter((d) => d.isDirectory())
    .flatMap((v) => fs.readdirSync(path.join(root, v.name), { withFileTypes: true })
      .filter((d) => d.isDirectory() && fs.existsSync(path.join(root, v.name, d.name, 'meta.json')))
      .map((d) => d.name));
}

/** GET a JSON endpoint; returns { status, body }. */
export async function getJson(request, url) {
  const res = await request.get(url);
  const type = res.headers()['content-type'] || '';
  expect(type, `${url}: content type`).toContain('application/json');
  return { status: res.status(), body: await res.json() };
}

/** GET a JSON endpoint that must answer 200. */
export async function api(request, url) {
  const { status, body } = await getJson(request, url);
  expect(status, `${url}: ${JSON.stringify(body)}`).toBe(200);
  return body;
}

// ---- shapes ----
// A shape is { key: type }, type one of 'string' 'number' 'integer' 'boolean' 'object' 'array' 'null' 'any',
// several joined with '|', or a nested shape object. `[shape]` is an array whose items have that shape.

function typeOf(v) {
  if (v === null) return 'null';
  if (Array.isArray(v)) return 'array';
  return typeof v;
}

function checkType(v, t, where, errors) {
  if (Array.isArray(t)) {
    if (!Array.isArray(v)) { errors.push(`${where}: want array, got ${typeOf(v)}`); return; }
    v.forEach((x, i) => checkType(x, t[0], `${where}[${i}]`, errors));
    return;
  }
  if (typeof t === 'object') { checkShape(v, t, where, errors); return; }
  const ok = t.split('|').some((one) => one === 'any' || (one === 'integer' ? Number.isInteger(v) : typeOf(v) === one));
  if (!ok) errors.push(`${where}: want ${t}, got ${typeOf(v)} (${JSON.stringify(v)?.slice(0, 60)})`);
}

function checkShape(obj, shape, where, errors) {
  if (typeOf(obj) !== 'object') { errors.push(`${where}: want object, got ${typeOf(obj)}`); return; }
  const want = Object.keys(shape).sort(), got = Object.keys(obj).sort();
  for (const k of want) if (!(k in obj)) errors.push(`${where}: missing key ${k}`);
  for (const k of got) if (!(k in shape)) errors.push(`${where}: undocumented key ${k}`);
  for (const k of want) if (k in obj) checkType(obj[k], shape[k], `${where}.${k}`, errors);
}

/** Assert that `value` has exactly the keys of `shape` with the right types (recursively). */
export function expectShape(value, shape, where = '$') {
  const errors = [];
  checkType(value, shape, where, errors);
  expect(errors, `shape of ${where}`).toEqual([]);
}

/** A string -> string map. */
export function expectStringMap(obj, where) {
  expect(typeOf(obj), where).toBe('object');
  for (const [k, v] of Object.entries(obj)) expect(typeof v, `${where}.${k}`).toBe('string');
}

const RUN_SUMMARY = {
  run_id: 'string', folder: 'string', label: 'string', group: 'string', arch: 'string',
  nproc: 'integer|null', ram_gb: 'number|null', cpu_model: 'string|null', vmbench_version: 'string', start_utc: 'string',
  source: 'string', catalog_known: 'boolean', failed_stages: ['string'], tags: 'object', values: 'integer',
  kept_files: ['string'],
};
const SIDE = { reference: 'string', label: 'string', run_ids: ['string'], archs: ['string'] };

export const SHAPES = {
  health: { benchlens: 'string', db_layout: 'integer', runs: 'integer' },
  runSummary: RUN_SUMMARY,
  runDetail: {
    ...RUN_SUMMARY, stages: 'string|null', end_utc: 'string|null', imported_at: 'string', pg_profile: 'string|null',
    web_cpus: 'string|null', meta: 'object', hardware: 'object|null', files: [{ name: 'string', bytes: 'integer' }],
  },
  group: {
    group: 'string', runs: 'integer', labels: ['string'], archs: ['string'], nproc: ['integer'], cpu_models: ['string'],
    vmbench_versions: ['string'], first_start_utc: 'string', last_start_utc: 'string',
  },
  side: SIDE,
  row: {
    other: 'integer', id: 'string', name: 'string', section: 'string', unit: 'string', direction: 'string',
    base: 'number', value: 'number', cv_base: 'number|null', cv_value: 'number|null', n_base: 'integer', n_value: 'integer',
    diff: 'number|null', noise: 'number', verdict: 'string', cross_arch: 'boolean',
  },
  tally: { better: 'integer', worse: 'integer', not_clear: 'integer', by_section: 'object' },
  valueRow: {
    run_id: 'string', id: 'string', name: 'string', section: 'string', unit: 'string', direction: 'string',
    tool: 'string|null', arch: 'string', round: 'integer', value: 'number',
  },
  metric: { id: 'string', unit: 'string', direction: 'string', tool: 'string', label: 'string', description: 'string' },
  error: { error: 'string', detail: 'string' },
};

/** `{error, detail?}` with the given status. */
export async function expectApiError(request, url, status) {
  const res = await getJson(request, url);
  expect(res.status, `${url}: ${JSON.stringify(res.body)}`).toBe(status);
  const shape = 'detail' in res.body ? SHAPES.error : { error: 'string' };
  expectShape(res.body, shape, url);
  expect(res.body.error.length, `${url}: error message`).toBeGreaterThan(0);
  return res.body;
}

// ---- the comparisons under test ----

/** Query string of a comparison: base=...&other=... */
export const sidesQuery = (cmp) => new URLSearchParams([['base', cmp.base], ...cmp.others.map((o) => ['other', o])]).toString();

/**
 * Two comparisons picked from what the server holds:
 * - groups across architectures: the two biggest single-architecture groups whose architectures differ;
 * - runs of one group: the two runs with the most values in the biggest group.
 */
export async function pickComparisons(request) {
  const groups = await api(request, 'api/groups');
  const runs = await api(request, 'api/runs');
  const single = groups.filter((g) => g.archs.length === 1).sort((a, b) => b.runs - a.runs || a.group.localeCompare(b.group));
  const g1 = single[0];
  const g2 = single.find((g) => g.archs[0] !== g1.archs[0]);
  expect(g2, 'the fixtures have groups on two architectures').toBeTruthy();
  const big = [...groups].sort((a, b) => b.runs - a.runs || a.group.localeCompare(b.group))[0];
  expect(big.runs, 'the fixtures have a group with two runs').toBeGreaterThanOrEqual(2);
  const inBig = runs.filter((r) => r.group === big.group)
    .sort((a, b) => b.values - a.values || a.start_utc.localeCompare(b.start_utc));
  return [
    { name: 'groups across architectures', base: 'group:' + g1.group, others: ['group:' + g2.group] },
    { name: 'runs of one group', base: inBig[0].run_id, others: [inBig[1].run_id] },
  ];
}

// ---- formats the UI documents (web/js/format.js): checked against the API's numbers ----

/** A difference in percent as the heatmap shows it: +12%, −3.4% (U+2212 minus), — for none. */
export function pct(d) {
  if (d == null || !Number.isFinite(d)) return '—';
  const a = Math.abs(d);
  return (d > 0 ? '+' : d < 0 ? '−' : '') + (a >= 10 ? a.toFixed(0) : a.toFixed(1)) + '%';
}

export const VERDICT_TEXT = { better: 'better', worse: 'worse', not_clear: 'within noise' };

// ---- page fixture: every console error or page error fails the test ----

export const test = base.extend({
  pageErrors: async ({}, use) => { await use([]); },
  page: async ({ page, pageErrors }, use) => {
    watchErrors(page, pageErrors);
    await use(page);
    expect(pageErrors, 'console and page errors').toEqual([]);
  },
});

export function watchErrors(page, into) {
  page.on('console', (m) => { if (m.type() === 'error') into.push(`console: ${m.text()} (${m.location().url})`); });
  page.on('pageerror', (e) => into.push(`page error: ${e.message}`));
  page.on('response', (r) => { if (r.status() >= 400) into.push(`HTTP ${r.status()} ${r.url()}`); });
}

// ---- waiting for views ----

/** Open a UI URL (query string) and wait until the view named in it has rendered its data. */
export async function openView(page, query) {
  await page.goto(query);
  const view = new URLSearchParams(query.replace(/^[^?]*\?/, '')).get('view') || 'summary';
  await waitForView(page, view);
  return view;
}

export async function waitForView(page, view) {
  await expect(page.locator('#view-' + view)).toBeVisible();
  await expect(page.locator('.state.loading')).toHaveCount(0);
  if (view === 'compare') await expect(page.locator('#heat tbody tr').first()).toBeVisible();
  if (view === 'metric') await expect(page.locator('#metric-stats tbody tr').first()).toBeVisible();
  if (view === 'runs') await expect(page.locator('#runs-table tbody tr').first()).toBeVisible();
  await expect(page.locator('.state.error')).toHaveCount(0);
}

/**
 * Horizontal overflow of the page: how far the document scrolls sideways, and elements that reach past the
 * right edge of the viewport outside a box that scrolls or clips on its own (such content would be cut off
 * if a page style ever hid the overflow).
 */
export async function sidewaysOverflow(page) {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const scroll = Math.max(document.documentElement.scrollWidth, document.body.scrollWidth) - vw;
    const clipped = [];
    const boxed = (el) => {
      for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
        const o = getComputedStyle(p).overflowX;
        if (o !== 'visible') return true;
      }
      return false;
    };
    for (const el of document.querySelectorAll('body *')) {
      if (el.closest('[hidden], .sr-only, dialog:not([open]), #tip')) continue;
      const r = el.getBoundingClientRect();
      if (!r.width || !r.height) continue;
      if (r.right > vw + 1 && !boxed(el)) {
        const name = el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + (el.className && typeof el.className === 'string' ? '.' + el.className.trim().split(/\s+/).join('.') : '');
        clipped.push(`${name} right=${Math.round(r.right)}`);
      }
    }
    return { scroll, clipped: clipped.slice(0, 10) };
  });
}
