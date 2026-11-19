// Every view at 1280 and 400 px, light and dark: renders its data, no console or page errors, no failed
// request, no sideways page scroll and nothing cut off at the right edge. A screenshot of each is kept in
// test-results/ (uploaded by CI when something fails).

import { test } from '@playwright/test';
import { expect, api, pickComparisons, sidesQuery, watchErrors, waitForView, sidewaysOverflow } from './helpers.js';

const WIDTHS = [1280, 400];
const SCHEMES = ['light', 'dark'];

for (const scheme of SCHEMES) {
  for (const width of WIDTHS) {
    test(`every view at ${width}px, ${scheme}`, async ({ browser, request }, testInfo) => {
      const [cmp] = await pickComparisons(request);
      const runs = await api(request, 'api/runs');
      const c = await api(request, 'api/compare?' + sidesQuery(cmp));
      const sides = sidesQuery(cmp);
      const cross = c.rows.find((r) => r.cross_arch);
      const pages = [
        ['compare', `?${sides}`],
        ['compare-all', `?${sides}&section=all`],
        ['metric', `?view=metric&${sides}&metric=${encodeURIComponent(c.rows[0].id)}`],
        ...(cross ? [['metric-cross-arch', `?view=metric&${sides}&metric=${encodeURIComponent(cross.id)}`]] : []),
        ['runs', '?view=runs'],
        ['hardware', `?view=hardware&${sides}`],
      ];
      const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
      const page = await ctx.newPage();
      const errors = [];
      watchErrors(page, errors);
      const problems = [];
      try {
        for (const [name, q] of pages) {
          const view = name.split('-')[0];
          await page.goto(q);
          await waitForView(page, view);
          if (view === 'compare') await expect(page.locator('#tally tbody tr')).toHaveCount(c.others.length);
          if (view === 'metric') await expect(page.locator('#dots svg circle').first()).toBeVisible();
          if (view === 'runs') await expect(page.locator('#runs-table tbody tr[data-run]')).toHaveCount(runs.length);
          if (view === 'hardware') {
            await page.click('#hw-from-compare');
            const ids = new Set([c.base, ...c.others].flatMap((s) => s.run_ids));
            await expect(page.locator('#hw-table thead th')).toHaveCount(ids.size + 1);
          }
          const scheme_ = await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches);
          expect(scheme_, 'colour scheme applied').toBe(scheme === 'dark');
          const o = await sidewaysOverflow(page);
          if (o.scroll > 0) problems.push(`${name}: page scrolls sideways by ${o.scroll}px`);
          if (o.clipped.length) problems.push(`${name}: content past the right edge: ${o.clipped.join(', ')}`);
          await page.screenshot({ path: testInfo.outputPath(`${name}-${width}-${scheme}.png`), fullPage: true });
        }
      } finally {
        await ctx.close();
      }
      expect(problems, 'layout').toEqual([]);
      expect(errors, 'console and page errors').toEqual([]);
    });
  }
}

// UI bug (docs/reviews/wave1-b.md, item 2): the Runs view links report.md for every run, but the fixtures keep
// none, so the link opens a 404. Enable when the link is shown only for runs whose `files` list report.md.
test.fixme('runs: report.md is linked only for runs that kept it', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  await page.goto('?view=runs');
  await waitForView(page, 'runs');
  for (const r of runs) {
    const d = await api(request, 'api/runs/' + r.run_id);
    const has = d.files.some((f) => f.name === 'report.md');
    await expect(page.locator(`#runs-table tr[data-run="${r.run_id}"] a`, { hasText: 'report.md' })).toHaveCount(has ? 1 : 0);
  }
});
