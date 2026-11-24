// Summary view, block "Verdicts by section": the counts are the tallies of /api/compare, the same numbers the
// CLI prints (`benchlens compare --json`), in the bars' labels, the tooltip text and the table fallback.

import { test, expect, api, pickComparisons, sidesQuery, sidewaysOverflow } from './helpers.js';

async function openVerdicts(page, cmp) {
  await page.goto(`?view=summary&${sidesQuery(cmp)}`);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-status .state')).toHaveCount(0);
  await expect(page.locator('#sum-verdicts')).toBeVisible();
  await expect(page.locator('#sum-verdicts .sum-verdicts-block').first()).toBeVisible();
}

/** The table fallback as text: [[side, section, better, worse, within noise, results]]. */
const tableRows = (page) => page.locator('#sum-verdicts table.data tbody tr').evaluateAll(
  (trs) => trs.map((tr) => [...tr.children].map((c) => c.textContent)));

const SECTION = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', pg: 'PostgreSQL', net: 'Network', web: 'Web' };

for (const which of [0, 1]) {
  test(`summary verdicts: counts equal the tallies of /api/compare (comparison ${which})`, async ({ page, request }) => {
    const cmp = (await pickComparisons(request))[which];
    const c = await api(request, `api/compare?${sidesQuery(cmp)}`);
    await openVerdicts(page, cmp);

    const rows = await tableRows(page);
    c.tallies.forEach((t, i) => {
      const label = c.others[i].label;
      const mine = rows.filter((r) => r[0] === label);
      // every section of the tally has a row with exactly its three counts
      for (const [sec, [b, w, n]] of Object.entries(t.by_section)) {
        const row = mine.find((r) => r[1] === (SECTION[sec] || sec));
        expect(row, `${label} / ${sec}`).toBeTruthy();
        expect(row.slice(2).map(Number)).toEqual([b, w, n, b + w + n]);
      }
      const all = mine.find((r) => r[1] === 'All sections');
      expect(all.slice(2).map(Number)).toEqual([t.better, t.worse, t.not_clear, t.better + t.worse + t.not_clear]);
      expect(mine.length).toBe(Object.keys(t.by_section).length + 1);
    });

    // the chart says the same: one chart per side, its accessible name carries the totals
    const charts = page.locator('#sum-verdicts .sum-verdicts-block svg');
    await expect(charts).toHaveCount(c.others.length);
    for (let i = 0; i < c.tallies.length; i++) {
      const t = c.tallies[i];
      await expect(charts.nth(i)).toHaveAttribute('aria-label', new RegExp(`better ${t.better} · worse ${t.worse} · within noise ${t.not_clear}$`));
      await expect(page.locator('#sum-verdicts .sum-verdicts-block').nth(i).locator('h3')).toHaveText(c.others[i].label);
    }
  });
}

test('summary verdicts: a bar shows its counts as text, and a tooltip on hover', async ({ page, request }) => {
  const cmp = (await pickComparisons(request))[0];
  const c = await api(request, `api/compare?${sidesQuery(cmp)}`);
  await openVerdicts(page, cmp);
  const [sec, [b, w]] = Object.entries(c.tallies[0].by_section).find(([, v]) => v[0] + v[1] > 0);
  const row = page.locator('#sum-verdicts .sum-verdicts-row').filter({ hasText: SECTION[sec] || sec }).first();
  const texts = await row.locator('text').allTextContents();
  expect(texts.some((t) => t === String(b))).toBe(true);
  expect(texts.some((t) => t === String(w))).toBe(true);
  await row.hover();
  await expect(page.locator('#tip')).toContainText(`better ${b} · worse ${w}`);
  // colour is not the only cue: the worse bars are hatched
  await expect(page.locator('#sum-verdicts .sum-verdicts-hatch').first()).toBeAttached();
});

test('summary verdicts: the sysbench note shows exactly when a row is not counted across architectures', async ({ page, request }) => {
  for (const cmp of await pickComparisons(request)) {
    const c = await api(request, `api/compare?${sidesQuery(cmp)}`);
    await openVerdicts(page, cmp);
    const cross = c.rows.some((r) => r.cross_arch);
    await expect(page.locator('#sum-verdicts .sum-verdicts-note')).toHaveCount(cross ? 1 : 0);
    if (cross) await expect(page.locator('#sum-verdicts .sum-verdicts-note')).toContainText('sysbench is not counted across architectures');
  }
});

test('summary verdicts: the table fallback is closed by default and opens', async ({ page, request }) => {
  const cmp = (await pickComparisons(request))[0];
  await openVerdicts(page, cmp);
  const details = page.locator('#sum-verdicts details.sum-table');
  await expect(details).not.toHaveAttribute('open', '');
  await details.locator('summary').click();
  await expect(details.locator('table.data tbody tr').first()).toBeVisible();
});

test('summary verdicts: no sideways scroll at 400px', async ({ browser, request }) => {
  const cmp = (await pickComparisons(request))[0];
  const ctx = await browser.newContext({ viewport: { width: 400, height: 900 } });
  const page = await ctx.newPage();
  try {
    await openVerdicts(page, cmp);
    const o = await sidewaysOverflow(page);
    expect(o.scroll).toBe(0);
    expect(o.clipped).toEqual([]);
  } finally {
    await ctx.close();
  }
});

test('summary verdicts: dark theme keeps the bars visible', async ({ browser, request }) => {
  const cmp = (await pickComparisons(request))[0];
  const ctx = await browser.newContext({ colorScheme: 'dark' });
  const page = await ctx.newPage();
  try {
    await openVerdicts(page, cmp);
    const fills = await page.locator('#sum-verdicts .sum-verdicts-better, #sum-verdicts .sum-verdicts-worse').first()
      .evaluate((e) => getComputedStyle(e).fill);
    expect(fills).not.toBe('none');
    expect(fills).not.toBe(await page.evaluate(() => getComputedStyle(document.body).backgroundColor));
  } finally {
    await ctx.close();
  }
});
