// The Export button of the compare view downloads the static HTML report of the current sides, the same file
// as GET /api/export.html. tests/export/run.sh (run by scripts/e2e.sh after this suite) opens such a file from
// disk with the network off and checks its numbers against `benchlens compare --json`.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test, expect, openView, pickComparisons, sidesQuery } from './helpers.js';

test('export: the button downloads the report of the shown sides', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  await openView(page, '?view=compare&' + sidesQuery(cmp));
  const download = page.waitForEvent('download');
  await page.locator('#export-html').click();
  const d = await download;
  expect(d.suggestedFilename()).toMatch(/^benchlens-.+-vs-.+\.html$/);
  const html = fs.readFileSync(await d.path(), 'utf8');
  const json = html.match(/<script type="application\/json" id="benchlens-data">([^<]*)<\/script>/);
  expect(json, 'inlined data').toBeTruthy();
  const data = JSON.parse(json[1]);
  expect(data.base).toBe(cmp.base);
  expect(data.others).toEqual(cmp.others);
  expect(data.sides.map((s) => s.side.reference)).toEqual([cmp.base, ...cmp.others]);
  await expect(page.locator('#export-status')).toHaveText('');

  const res = await request.get('api/export.html?' + sidesQuery(cmp));
  expect(res.status()).toBe(200);
  expect(res.headers()['content-type']).toContain('text/html');
  expect(res.headers()['content-disposition']).toContain('attachment');
  expect(res.headers()['content-security-policy']).toContain("default-src 'none'");
});

// The exported file opens on the Summary view and shows what the live Summary shows, with the network off.
test('export: the file opens on the Summary view offline and its numbers equal the live view', async ({ page, request, browser }) => {
  const [cmp] = await pickComparisons(request);
  const query = '?view=summary&' + sidesQuery(cmp);
  const readSummary = (p) => p.evaluate(() => ({
    board: [...document.querySelectorAll('#sum-board .sum-board-row')].map((r) => [
      r.querySelector('.sum-board-name').textContent, r.querySelector('.sum-board-value').textContent, r.querySelector('.sum-board-detail').textContent]),
    verdicts: [...document.querySelectorAll('#sum-verdicts table.data tbody tr')].map((tr) => [...tr.children].map((c) => c.textContent)),
    hidden: ['sum-board', 'sum-radar', 'sum-price', 'sum-scaling', 'sum-stability', 'sum-verdicts'].filter((id) => document.getElementById(id).hidden),
    notes: [...document.querySelectorAll('#sum-hidden-notes .sum-note')].map((n) => n.textContent),
  }));
  await openView(page, query);
  await expect(page.locator('#sum-board .sum-board-row').first()).toBeVisible();
  await expect(page.locator('#sum-verdicts table.data')).toBeAttached();
  const live = await readSummary(page);

  await openView(page, '?view=compare&' + sidesQuery(cmp));
  const download = page.waitForEvent('download');
  await page.locator('#export-html').click();
  // Chromium keeps downloads under a bare id; the browser needs ".html" to show the file as a page
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'benchlens-export-'));
  const file = path.join(dir, 'report.html');
  await (await download).saveAs(file);

  const ctx = await browser.newContext({ offline: true });
  const requests = [];
  await ctx.route('**/*', (route) => {
    const u = route.request().url();
    if (u.startsWith('file:')) return route.continue();
    requests.push(u);
    return route.abort();
  });
  const off = await ctx.newPage();
  const errors = [];
  off.on('pageerror', (e) => errors.push(e.message));
  off.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await off.goto('file://' + file);
  await expect(off.locator('#view-summary')).toBeVisible();
  await expect(off.locator('nav a[aria-current="page"]')).toHaveAttribute('data-view', 'summary');
  await expect(off.locator('#sum-board .sum-board-row').first()).toBeVisible();
  await expect(off.locator('#sum-verdicts table.data')).toBeAttached();
  const exported = await readSummary(off);
  expect(exported.board.length).toBeGreaterThan(1);
  expect(exported.board).toEqual(live.board);
  expect(exported.verdicts).toEqual(live.verdicts);
  // the same blocks are shown or hidden, with the same reasons
  expect(exported.hidden).toEqual(live.hidden);
  expect(exported.notes).toEqual(live.notes);
  await ctx.close();
  fs.rmSync(dir, { recursive: true, force: true });
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});

test('export: the file carries the stylesheet of the export once', async ({ request }) => {
  const [cmp] = await pickComparisons(request);
  const res = await request.get('api/export.html?' + sidesQuery(cmp));
  const html = await res.text();
  expect(html.split('.export-meta { margin-bottom: 16px; }').length - 1).toBe(1);
  expect(html.split('<style>').length - 1).toBe(2); // css/app.css, css/summary.css (+ export.css appended to the last)
  expect(html).toContain('.sum-'); // css/summary.css is inlined
});
