// Summary block 1, the leaderboard with workload profiles: the bars, their order and the verdict counts equal
// what score.js and /api/compare give for the same comparison, the profile control writes the URL, a link with
// profile=custom&w=... restores the same chart, and invalid custom weights show an inline error.

import { test, expect, api, pickComparisons, sidesQuery, sidewaysOverflow } from './helpers.js';
import { weightsFor, scoresFor, formatWeights } from '../../../web/js/score.js';

let comparisons = [];
test.beforeAll(async ({ request }) => { comparisons = await pickComparisons(request); });

const params = (page) => new URL(page.url()).searchParams;

async function openBoard(page, cmp, extra = '') {
  await page.goto(`?view=summary&${sidesQuery(cmp)}${extra}`);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-board .sum-board-row').first()).toBeVisible();
}

/** What the page must show: sides (baseline included) best first, with score.js numbers and tallies. */
function expectedBoard(c, profile, w = '') {
  const scores = scoresFor(c, weightsFor(profile, w).weights);
  const entries = [{ name: scores.base.label + ' (baseline)', score: 100, detail: 'baseline' }];
  scores.sides.forEach((s, i) => {
    const t = c.tallies[i];
    entries.push({ name: s.label, score: s.overall.score, detail: `better ${t.better} · worse ${t.worse} · within noise ${t.not_clear}` });
  });
  const key = (e) => (e.score == null ? -Infinity : e.score);
  return entries.map((e, i) => ({ e, i })).sort((a, b) => key(b.e) - key(a.e) || a.i - b.i).map((x) => x.e);
}

async function shown(page) {
  return page.$$eval('#sum-board .sum-board-row', (rows) => rows.map((r) => ({
    name: r.querySelector('.sum-board-name').textContent,
    value: r.querySelector('.sum-board-value').textContent,
    detail: r.querySelector('.sum-board-detail').textContent,
  })));
}

for (const i of [0, 1]) {
  test(`summary board: bars, order and counts equal score.js and /api/compare (comparison ${i + 1})`, async ({ page, request }) => {
    const cmp = comparisons[i];
    const c = await api(request, 'api/compare?' + sidesQuery(cmp));
    for (const profile of ['balanced', 'db', 'compute']) {
      await openBoard(page, cmp, profile === 'balanced' ? '' : `&profile=${profile}`);
      const want = expectedBoard(c, profile);
      const got = await shown(page);
      expect(got.map((g) => g.name), profile).toEqual(want.map((e) => e.name));
      got.forEach((g, k) => {
        if (want[k].score == null) expect(g.value).toBe('no score');
        else expect(Math.abs(Number(g.value) - want[k].score), `${profile} ${g.name}`).toBeLessThanOrEqual(0.051 + (Math.abs(want[k].score - 100) < 10 ? 0 : 0.5));
        expect(g.detail).toBe(want[k].detail);
      });
      await expect(page.locator('#sum-board .sum-board-takeaway')).toContainText('For ');
    }
  });
}

test('summary board: "Show as table" repeats the numbers of the bars', async ({ page }) => {
  await openBoard(page, comparisons[0]);
  const bars = await shown(page);
  await page.locator('#sum-board details.sum-table summary').click();
  const rows = await page.$$eval('#sum-board details.sum-table tbody tr', (trs) => trs.map((tr) => [...tr.children].map((c) => c.textContent)));
  expect(rows.map((r) => r[0])).toEqual(bars.map((b) => b.name));
  expect(rows.map((r) => r[1])).toEqual(bars.map((b) => b.value));
});

test('summary board: the radio buttons write profile to the URL and re-render the page', async ({ page, request }) => {
  const cmp = comparisons[0];
  const c = await api(request, 'api/compare?' + sidesQuery(cmp));
  await openBoard(page, cmp);
  await expect(page.locator('input[name="sum-profile"]:checked')).toHaveValue('balanced');
  await page.locator('input[name="sum-profile"][value="db"]').check();
  await expect.poll(() => params(page).get('profile')).toBe('db');
  await expect(page.locator('input[name="sum-profile"]:checked')).toHaveValue('db');
  const want = expectedBoard(c, 'db');
  await expect.poll(async () => (await shown(page)).map((g) => g.name)).toEqual(want.map((e) => e.name));
  await expect(page.locator('#sum-board .sum-board-takeaway')).toContainText('Database');
  await page.locator('input[name="sum-profile"][value="balanced"]').check();
  await expect.poll(() => params(page).get('profile')).toBeNull();
});

test('summary board: custom weights are validated, written to the URL and restored from a link', async ({ page, request }) => {
  const cmp = comparisons[0];
  const c = await api(request, 'api/compare?' + sidesQuery(cmp));
  await openBoard(page, cmp);
  await page.locator('input[name="sum-profile"][value="custom"]').check();
  await expect.poll(() => params(page).get('profile')).toBe('custom');
  await expect(page.locator('#sum-w-cpu')).toBeVisible();

  // each weight applies when its input loses the focus. Invalid: negative, then all zero; the URL keeps the last
  // valid weights (moving from one input to the next applies the one left, while it is still valid)
  const before = params(page).get('w');
  await page.locator('#sum-w-cpu').fill('-1');
  await page.locator('#sum-w-cpu').blur();
  await expect(page.locator('.sum-board-custom .sum-board-error')).toContainText('CPU: a weight cannot be negative');
  await expect(page.locator('#sum-w-cpu')).toHaveAttribute('aria-invalid', 'true');
  expect(params(page).get('w')).toBe(before);
  for (const sec of ['cpu', 'disk', 'mem', 'net', 'pg', 'web']) await page.locator(`#sum-w-${sec}`).fill('0');
  await page.locator('#sum-w-web').blur();
  await expect(page.locator('.sum-board-custom .sum-board-error')).toContainText('At least one');
  expect(params(page).get('w')).toBe('cpu:0,disk:0,mem:0,net:0,pg:0,web:1');
  // what was typed survives the re-renders in between
  for (const sec of ['cpu', 'disk', 'mem', 'net', 'pg', 'web']) await expect(page.locator(`#sum-w-${sec}`)).toHaveValue('0');

  // valid: written in the fixed section order
  await page.locator('#sum-w-cpu').fill('3');
  await page.locator('#sum-w-mem').fill('2');
  await page.locator('#sum-w-mem').blur();
  const w = 'cpu:3,disk:0,mem:2,net:0,pg:0,web:0';
  await expect.poll(() => params(page).get('w')).toBe(w);
  await expect(page.locator('.sum-board-custom .sum-board-error')).toBeHidden();

  // a link restores the same chart
  const first = await shown(page);
  await openBoard(page, cmp, `&profile=custom&w=${encodeURIComponent(w)}`);
  expect(await shown(page)).toEqual(first);
  await expect(page.locator('#sum-w-cpu')).toHaveValue('3');
  const want = expectedBoard(c, 'custom', w);
  expect((await shown(page)).map((g) => g.name)).toEqual(want.map((e) => e.name));
  expect(formatWeights(weightsFor('custom', w).weights)).toBe(w);
});

test('summary board: the keyboard focus stays on the control used, across the re-render it causes', async ({ page }) => {
  await openBoard(page, comparisons[0]);
  const focusedId = () => page.evaluate(() => document.activeElement && document.activeElement.id);
  // arrow keys move through the radio group: each step changes the profile and re-renders every block
  await page.locator('#sum-profile-balanced').focus();
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => params(page).get('profile')).toBe('web');
  await expect(page.locator('#sum-board .sum-board-takeaway')).toContainText('Web app');
  await expect.poll(focusedId).toBe('sum-profile-web');
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => params(page).get('profile')).toBe('db');
  await expect.poll(focusedId).toBe('sum-profile-db');
  // custom weights: Tab from one input to the next applies the first and keeps the focus on the second
  await page.locator('#sum-profile-custom').check();
  await expect.poll(() => params(page).get('profile')).toBe('custom');
  await expect.poll(focusedId).toBe('sum-profile-custom');
  await page.locator('#sum-w-cpu').fill('4');
  await page.keyboard.press('Tab');
  await expect.poll(() => params(page).get('w')).toMatch(/^cpu:4,/);
  await expect.poll(focusedId).toBe('sum-w-disk');
});

test('summary board: an invalid w in a link is reported and Balanced is shown', async ({ page }) => {
  await openBoard(page, comparisons[0], '&profile=custom&w=nonsense');
  await expect(page.locator('#sum-board .sum-board-error').first()).toContainText('not valid');
  await expect(page.locator('input[name="sum-profile"]:checked')).toHaveValue('custom');
});

test('summary board: no sideways scroll at 400px, and the baseline is marked by a label, not only by colour', async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 400, height: 900 } });
  const page = await ctx.newPage();
  try {
    await openBoard(page, comparisons[0], '&profile=custom&w=cpu:1,disk:3,mem:2,net:0.5,pg:3,web:1');
    const o = await sidewaysOverflow(page);
    expect(o.scroll).toBe(0);
    expect(o.clipped).toEqual([]);
    await expect(page.locator('#sum-board .sum-board-row[data-baseline="true"] .sum-board-name')).toContainText('(baseline)');
  } finally {
    await ctx.close();
  }
});
