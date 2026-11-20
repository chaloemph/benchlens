// The Export button of the compare view downloads the static HTML report of the current sides, the same file
// as GET /api/export.html. tests/export/run.sh (run by scripts/e2e.sh after this suite) opens such a file from
// disk with the network off and checks its numbers against `benchlens compare --json`.

import fs from 'node:fs';
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
