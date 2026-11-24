// Summary view, Scaling block: lines per architecture over the vCPU counts of the sides, hidden when no
// architecture has two sizes. Real fixtures (scripts/e2e.sh imports them with the POC registry): x86-std has
// 2 vCPUs, x86-high and arm64-high 16. The groups are picked from /api/groups at run time. The expected
// scores are computed with web/js/score.js from /api/compare, the same module the page uses.

import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { test, expect, api, sidesQuery, sidewaysOverflow } from './helpers.js';

const score = await import(pathToFileURL(path.resolve(import.meta.dirname, '../../../web/js/score.js')).href);

/** Same-arch pair with different vCPU counts (small, big) and a group of another arch (other). */
async function pickSizes(request) {
  const groups = (await api(request, 'api/groups')).filter((g) => g.archs.length === 1 && g.nproc.length === 1);
  for (const small of groups) {
    const big = groups.find((g) => g.archs[0] === small.archs[0] && g.nproc[0] > small.nproc[0]);
    const other = groups.find((g) => g.archs[0] !== small.archs[0]);
    if (big && other) return { small, big, other };
  }
  throw new Error('the fixtures need two groups of one architecture with different vCPU counts and a group of another architecture');
}

const ref = (g) => 'group:' + g.group;
const compareUrl = (base, others) => 'api/compare?' + sidesQuery({ base: ref(base), others: others.map(ref) });

async function openSummary(page, cmp, extra = '') {
  await page.goto(`?view=summary&${sidesQuery(cmp)}${extra}`);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-status .state')).toHaveCount(0);
  await expect(page.locator('#sum-body')).toBeVisible();
  await expect.poll(() => page.evaluate(() => {
    const el = document.getElementById('sum-scaling');
    return el.hidden || el.childElementCount > 0;
  })).toBe(true);
}

/** `{label: [cell, ...]}` of the table fallback (opened first). */
async function tableRows(page) {
  await page.locator('#sum-scaling details.sum-table summary').click();
  return page.locator('#sum-scaling details.sum-table tbody tr').evaluateAll((trs) => Object.fromEntries(
    trs.map((tr) => { const c = [...tr.children].map((x) => x.textContent); return [c[0], c]; })));
}

// table columns: Side, Architecture, vCPU, Score (summary), Linear expectation, Efficiency, Better, Worse, Within noise, Note
for (const profile of ['balanced', 'db']) {
  test(`scaling: small and big size of one arch draw a line, numbers equal score.js (${profile})`, async ({ page, request }) => {
    const { small, big, other } = await pickSizes(request);
    const cmp = { base: ref(small), others: [ref(big), ref(other)] };
    const comparison = await api(request, compareUrl(small, [big, other]));
    const scores = score.scoresFor(comparison, score.weightsFor(profile).weights);
    await openSummary(page, cmp, profile === 'balanced' ? '' : `&profile=${profile}`);

    const block = page.locator('#sum-scaling');
    await expect(block).toBeVisible();
    await expect(block.locator('svg').first()).toBeVisible();
    // one line (path) for the arch with two sizes, a dotted linear reference for it, no line for the lone arch
    await expect(block.locator('path.sum-sc-line')).toHaveCount(1);
    await expect(block.locator('line.sum-sc-ref').first()).toBeVisible();
    // x axis ticks at the real vCPU counts only
    const ticks = await block.locator('svg g.axis').first().locator('.tick text').allTextContents();
    expect([...new Set(ticks)].sort()).toEqual([...new Set([small.nproc[0], big.nproc[0]].map(String))].sort());
    await expect(block.locator('svg').first()).toContainText('Score (summary)');

    const rows = await tableRows(page);
    expect(Object.keys(rows)).toHaveLength(3);
    const bigRow = rows[big.group];
    expect(bigRow[1]).toBe(big.archs[0]);
    expect(Number(bigRow[2])).toBe(big.nproc[0]);
    expect(bigRow[3]).toBe(scores.sides[0].overall.score.toFixed(1));
    // efficiency = score / (baseline score x vCPU ratio): the baseline is the smallest point and scores 100
    const eff = (100 * scores.sides[0].overall.score / (100 * big.nproc[0] / small.nproc[0])).toFixed(0) + '%';
    expect(bigRow[5]).toBe(eff);
    expect(rows[small.group + ' (baseline)'][3]).toBe('100.0');
    expect(rows[small.group + ' (baseline)'][5]).toBe('100%');
    // the lone arch has no linear reference
    expect(rows[other.group][3]).toBe(scores.sides[1].overall.score.toFixed(1));
    expect(rows[other.group][5]).toBe('');
    // the verdict counts behind a score are in the table
    const tally = comparison.tallies[0];
    expect(bigRow.slice(6, 9)).toEqual([tally.better, tally.worse, tally.not_clear].map(String));
  });
}

test('scaling: the tooltip of a point shows score, verdict counts and vCPU', async ({ page, request }) => {
  const { small, big } = await pickSizes(request);
  const cmp = { base: ref(small), others: [ref(big)] };
  const comparison = await api(request, compareUrl(small, [big]));
  await openSummary(page, cmp);
  const mark = page.locator('#sum-scaling path.sum-sc-mark:not(.is-base)').first();
  await mark.focus();
  const tip = page.locator('#tip');
  await expect(tip).toBeVisible();
  const t = comparison.tallies[0];
  await expect(tip).toContainText(`${big.nproc[0]} vCPU`);
  await expect(tip).toContainText(`better ${t.better} · worse ${t.worse} · within noise ${t.not_clear}`);
  await expect(tip).toContainText('score (summary)');
});

test('scaling: one size per architecture hides the block and says why', async ({ page, request }) => {
  const { big, other } = await pickSizes(request);
  const cmp = { base: ref(big), others: [ref(other)] };
  await openSummary(page, cmp);
  await expect(page.locator('#sum-scaling')).toBeHidden();
  await expect(page.locator('#sum-scaling')).toBeEmpty();
  await expect(page.locator('#sum-hidden-notes .sum-note', { hasText: 'Scaling:' })).toHaveCount(1);
});

for (const width of [1280, 400]) {
  for (const scheme of ['light', 'dark']) {
    test(`scaling: no sideways scroll, readable in ${scheme} mode at ${width}px`, async ({ browser, request }) => {
      const { small, big, other } = await pickSizes(request);
      const cmp = { base: ref(small), others: [ref(big), ref(other)] };
      const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
      const page = await ctx.newPage();
      const errors = [];
      page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
      page.on('pageerror', (e) => errors.push(e.message));
      try {
        await openSummary(page, cmp);
        const block = page.locator('#sum-scaling');
        await expect(block.locator('svg').first()).toBeVisible();
        const box = await block.locator('.sum-sc-chart svg').boundingBox();
        expect(box.width).toBeLessThanOrEqual(width);
        // the line is drawn in a colour that differs from the page background, in both themes
        const [stroke, bg] = await block.locator('path.sum-sc-line').evaluate((el) => [getComputedStyle(el).stroke, getComputedStyle(document.body).backgroundColor]);
        expect(stroke).not.toBe(bg);
        expect(stroke).not.toBe('none');
        const o = await sidewaysOverflow(page);
        expect(o.scroll).toBe(0);
        expect(o.clipped).toEqual([]);
        expect(errors).toEqual([]);
      } finally {
        await ctx.close();
      }
    });
  }
}
