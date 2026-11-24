// Summary view, block "Price / performance". Prices are user input, not measurements, so they are never in the
// fixtures: every test sets "example prices" on fixture runs through PATCH /api/runs/{id}/tags and puts the
// `price` and `price_unit` tags back as they were (the suite runs one file after the other on one database).
//
// Numbers are checked against an independent value: the overall score comes from web/js/score.js (which
// tools/score_check.py recomputes in python, see tests/score/), and the index (score per price against the
// baseline = 100) is computed here with plain arithmetic, not with pricePerformance().

import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { test, expect, api, sidesQuery, watchErrors, sidewaysOverflow } from './helpers.js';

const score = await import(pathToFileURL(path.resolve(import.meta.dirname, '../../../web/js/score.js')).href);

const url = (cmp, extra = '') => `?view=summary&${sidesQuery(cmp)}${extra}`;
const f1 = (v) => v.toFixed(1);

/** The three groups the examples use: the biggest one (several runs) and two others, as base / other. */
async function groupsUnderTest(request) {
  const groups = await api(request, 'api/groups');
  const multi = [...groups].sort((a, b) => b.runs - a.runs || a.group.localeCompare(b.group))[0];
  expect(multi.runs, 'a group with several runs').toBeGreaterThanOrEqual(2);
  const rest = groups.filter((g) => g.group !== multi.group).sort((a, b) => a.group.localeCompare(b.group));
  expect(rest.length, 'two more groups').toBeGreaterThanOrEqual(2);
  return { multi: 'group:' + multi.group, a: 'group:' + rest[0].group, b: 'group:' + rest[1].group };
}

/** Sets the price tags of runs and puts them back (also when the test fails). */
function priceTags(request) {
  const original = new Map();
  return {
    async set(runIds, tags) {
      const runs = await api(request, 'api/runs');
      for (const id of runIds) {
        if (!original.has(id)) {
          const t = runs.find((r) => r.run_id === id).tags;
          original.set(id, { price: t.price ?? '', price_unit: t.price_unit ?? '' });
        }
        const res = await request.patch(`api/runs/${id}/tags`, { data: tags });
        expect(res.status(), `PATCH tags of ${id}`).toBe(200);
      }
    },
    async restore() {
      for (const [id, tags] of original) await request.patch(`api/runs/${id}/tags`, { data: tags });
      original.clear();
    },
  };
}

const runIdsOf = async (request, cmp, ref) => {
  const c = await api(request, 'api/compare?' + sidesQuery(cmp));
  return [c.base, ...c.others].find((s) => s.reference === ref).run_ids;
};

async function openSummary(page, query) {
  await page.goto(query);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-status .state')).toHaveCount(0);
  await expect(page.locator('#sum-body')).toBeVisible();
  await expect.poll(() => page.evaluate(() => ['sum-board', 'sum-price'].every((id) => {
    const el = document.getElementById(id);
    return el.hidden || el.childElementCount > 0;
  }))).toBe(true);
}

/** The rows of the "Show as table" fallback: [side, price, score, better, worse, within noise, index, frontier]. */
async function tableRows(page) {
  return page.locator('#sum-price details.sum-table tbody tr').evaluateAll((trs) => trs.map((tr) => [...tr.children].map((c) => c.textContent)));
}

test('price: hidden with no price and with one price, shown with two (example prices)', async ({ page, request }) => {
  const { a, b } = await groupsUnderTest(request);
  const cmp = { base: a, others: [b] };
  const prices = priceTags(request);
  try {
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price')).toBeHidden();
    await expect(page.locator('#sum-price')).toBeEmpty();
    await expect(page.locator('#sum-hidden-notes')).toContainText('Price / performance: add a "price" tag to the runs of at least two sides');

    await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: 'THB/month' });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price')).toBeHidden();
    await expect(page.locator('#sum-hidden-notes')).toContainText('Price / performance');

    await prices.set(await runIdsOf(request, cmp, b), { price: '6000', price_unit: 'THB/month' });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price')).toBeVisible();
    await expect(page.locator('#sum-price h2')).toHaveText('Price / performance');
    await expect(page.locator('#sum-price svg')).toBeVisible();
    await expect(page.locator('#sum-hidden-notes')).not.toContainText('Price / performance');
  } finally {
    await prices.restore();
  }
});

test('price: points, scores, counts and index equal the independent values; the unit is on the axis', async ({ page, request }) => {
  const { a, b } = await groupsUnderTest(request);
  const cmp = { base: a, others: [b] };
  const prices = priceTags(request);
  const profile = 'db';
  try {
    await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: 'THB/month' });
    await prices.set(await runIdsOf(request, cmp, b), { price: '6000', price_unit: 'THB/month' });
    await openSummary(page, url(cmp, `&profile=${profile}`));

    // expected: overall score of the profile from score.js; counts from the side's tally (as every block shows
    // them); the index is score per price against the baseline (100), plain arithmetic
    const c = await api(request, 'api/compare?' + sidesQuery(cmp));
    const sc = score.scoresFor(c, score.PROFILES[profile].weights);
    const side = sc.sides[0];
    const { better, worse, not_clear } = c.tallies[0];
    const counts = { better, worse, not_clear };
    const pb = 4000, po = 6000;
    const expBase = { price: String(pb), score: f1(100), index: f1(100) };
    const expOther = { price: String(po), score: f1(side.overall.score), index: f1(100 * (side.overall.score / po) / (100 / pb)) };

    const rows = await tableRows(page);
    expect(rows.length).toBe(2);
    const byName = Object.fromEntries(rows.map((r) => [r[0], r]));
    const rb = byName[c.base.label + ' (baseline)'];
    const ro = byName[c.others[0].label];
    expect(rb.slice(1, 3)).toEqual([expBase.price, expBase.score]);
    expect(rb[6]).toBe(expBase.index);
    expect(ro.slice(1, 6)).toEqual([expOther.price, expOther.score, String(counts.better), String(counts.worse), String(counts.not_clear)]);
    expect(ro[6]).toBe(expOther.index);

    // frontier: the same dominance rule written out here; one marker shape per class
    const pts = [{ p: pb, s: 100 }, { p: po, s: side.overall.score }];
    const front = pts.filter((e) => !pts.some((o) => o !== e && o.p <= e.p && o.s >= e.s && (o.p < e.p || o.s > e.s))).length;
    await expect(page.locator('#sum-price .sum-price-pt')).toHaveCount(2);
    await expect(page.locator('#sum-price .sum-price-pt.front')).toHaveCount(front);
    await expect(page.locator('#sum-price .sum-price-pt.front circle')).toHaveCount(front);
    await expect(page.locator('#sum-price .sum-price-pt.dominated rect')).toHaveCount(2 - front);

    // the chart says what it is: axis unit, "score (summary)", index list with the same index
    await expect(page.locator('#sum-price svg')).toContainText('Price (THB/month)');
    await expect(page.locator('#sum-price svg')).toContainText('Score (summary)');
    await expect(page.locator('#sum-price .sum-price-list li').first()).toContainText('score (summary)');
    const listed = await page.locator('#sum-price .sum-price-val').allTextContents();
    expect([...listed].sort()).toEqual([expBase.index, expOther.index].sort());

    // a point's tooltip carries the verdict counts behind its score
    await page.locator('#sum-price .sum-price-pt').nth(1).focus();
    await expect(page.locator('#tip')).toContainText(`better ${counts.better} · worse ${counts.worse} · within noise ${counts.not_clear}`);
  } finally {
    await prices.restore();
  }
});

test('price: the unit is shown as given, as text only', async ({ page, request }) => {
  const { a, b } = await groupsUnderTest(request);
  const cmp = { base: a, others: [b] };
  const prices = priceTags(request);
  const unit = '<i>THB</i>/month';
  try {
    await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: unit });
    await prices.set(await runIdsOf(request, cmp, b), { price: '0.5', price_unit: unit });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price svg')).toContainText(`Price (${unit})`);
    await expect(page.locator('#sum-price i')).toHaveCount(0);
    const rows = await tableRows(page);
    expect(rows.map((r) => r[1]).sort()).toEqual(['0.5', '4000']);
  } finally {
    await prices.restore();
  }
});

test('price: an inconsistent side is listed with its run labels and not plotted (example prices)', async ({ page, request }) => {
  const { multi, a, b } = await groupsUnderTest(request);
  const cmp = { base: a, others: [b, multi] };
  const prices = priceTags(request);
  try {
    await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: 'THB/month' });
    await prices.set(await runIdsOf(request, cmp, b), { price: '6000', price_unit: 'THB/month' });
    // the biggest group: its first run costs one amount, the others another
    const ids = await runIdsOf(request, cmp, multi);
    await prices.set(ids.slice(0, 1), { price: '5000', price_unit: 'THB/month' });
    await prices.set(ids.slice(1), { price: '9000', price_unit: 'THB/month' });
    await openSummary(page, url(cmp));

    const label = multi.replace(/^group:/, '');
    await expect(page.locator('#sum-price')).toBeVisible();
    await expect(page.locator('#sum-price .sum-price-pt')).toHaveCount(2);
    const problem = page.locator('#sum-price .sum-price-problems li');
    await expect(problem).toHaveCount(1);
    await expect(problem).toContainText(label);
    await expect(problem).toContainText('inconsistent price');
    // the run labels that differ, with their prices
    await expect(problem).toContainText('5000 THB/month');
    await expect(problem).toContainText('9000 THB/month');
    const rows = await tableRows(page);
    expect(rows.map((r) => r[0]).some((n) => n.startsWith(label))).toBe(false);
    expect(rows.length).toBe(2);
  } finally {
    await prices.restore();
  }
});

test('price: sides with another unit or an invalid price are listed, units are never mixed (example prices)', async ({ page, request }) => {
  const { multi, a, b } = await groupsUnderTest(request);
  const cmp = { base: a, others: [b, multi] };
  const prices = priceTags(request);
  try {
    await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: 'THB/month' });
    await prices.set(await runIdsOf(request, cmp, b), { price: '6000', price_unit: 'THB/month' });

    // another unit
    await prices.set(await runIdsOf(request, cmp, multi), { price: '0.1', price_unit: 'USD/hour' });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price svg')).toContainText('Price (THB/month)');
    await expect(page.locator('#sum-price .sum-price-pt')).toHaveCount(2);
    const other = page.locator('#sum-price .sum-price-problems li');
    await expect(other).toHaveCount(1);
    await expect(other).toContainText(multi.replace(/^group:/, ''));
    await expect(other).toContainText('different unit');
    await expect(other).toContainText('USD/hour');
    expect((await tableRows(page)).map((r) => r[1]).sort()).toEqual(['4000', '6000']);

    // an invalid price (a decimal comma) is shown and not used
    await prices.set(await runIdsOf(request, cmp, multi), { price: '1,5', price_unit: 'THB/month' });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price .sum-price-pt')).toHaveCount(2);
    await expect(page.locator('#sum-price .sum-price-problems li')).toContainText('invalid price');

    // no price at all on the third side
    await prices.set(await runIdsOf(request, cmp, multi), { price: '', price_unit: '' });
    await openSummary(page, url(cmp));
    await expect(page.locator('#sum-price .sum-price-problems li')).toContainText('no price tag');
  } finally {
    await prices.restore();
  }
});

for (const [width, scheme] of [[1280, 'light'], [400, 'light'], [400, 'dark']]) {
  test(`price: draws without errors or sideways scroll at ${width}px, ${scheme} (example prices)`, async ({ browser, request }) => {
    const { a, b } = await groupsUnderTest(request);
    const cmp = { base: a, others: [b] };
    const prices = priceTags(request);
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
    const page = await ctx.newPage();
    const errors = [];
    watchErrors(page, errors);
    try {
      await prices.set(await runIdsOf(request, cmp, a), { price: '4000', price_unit: 'THB/month' });
      await prices.set(await runIdsOf(request, cmp, b), { price: '6000', price_unit: 'THB/month' });
      await openSummary(page, url(cmp));
      await expect(page.locator('#sum-price svg')).toBeVisible();
      // the table fallback opens and shows both sides
      await page.locator('#sum-price details.sum-table summary').click();
      await expect(page.locator('#sum-price details.sum-table tbody tr')).toHaveCount(2);
      const o = await sidewaysOverflow(page);
      expect(o.scroll).toBe(0);
      expect(o.clipped).toEqual([]);
      expect(errors).toEqual([]);
    } finally {
      await ctx.close();
      await prices.restore();
    }
  });
}
