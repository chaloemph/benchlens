// Summary view shell: the view opens, the six block slots exist (placeholders until the blocks are built), the
// sides line leads to Compare with the same state, and `profile` and `w` survive a reload and the nav.

import { test, expect, pickComparisons, sidesQuery, openView, sidewaysOverflow } from './helpers.js';

const SLOTS = ['sum-board', 'sum-radar', 'sum-price', 'sum-scaling', 'sum-stability', 'sum-verdicts'];
const params = (page) => new URL(page.url()).searchParams;

async function openSummary(page, query) {
  await page.goto(query);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-status .state')).toHaveCount(0);
  for (const id of SLOTS) await expect(page.locator(`#${id}`)).toBeVisible();
}

test('summary: opens with the six slots in order, Summary first in the nav', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  await openSummary(page, `?view=summary&${sidesQuery(cmp)}`);
  const nav = page.locator('nav a[data-view]');
  await expect(nav.first()).toHaveAttribute('data-view', 'summary');
  await expect(nav.first()).toHaveAttribute('aria-current', 'page');
  await expect(page).toHaveTitle(/Summary/);
  const ids = await page.locator('#sum-body > *').evaluateAll((els) => els.map((e) => e.id));
  expect(ids).toEqual([...SLOTS, 'sum-hidden-notes']);
  await expect(page.locator('#sum-hidden-notes')).toHaveClass(/muted/);
  for (const id of SLOTS) await expect(page.locator(`#${id} .sum-placeholder`)).toHaveCount(1);
});

test('summary: sides line names the sides and links to Compare with the same state', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  await openSummary(page, `?view=summary&${sidesQuery(cmp)}`);
  const groups = await (await request.get('api/groups')).json();
  const baseName = cmp.base.replace(/^group:/, '');
  expect(groups.some((g) => g.group === baseName)).toBe(true);
  await expect(page.locator('#sum-sides')).toContainText(baseName);
  const change = page.locator('#sum-sides a');
  const href = new URL(await change.getAttribute('href'), page.url());
  expect(href.searchParams.get('view')).toBe('compare');
  expect(href.searchParams.get('base')).toBe(cmp.base);
  expect(href.searchParams.getAll('other')).toEqual(cmp.others);
  await change.click();
  await expect(page.locator('#view-compare')).toBeVisible();
  expect(params(page).get('base')).toBe(cmp.base);
  expect(params(page).getAll('other')).toEqual(cmp.others);
});

test('summary: profile and w round-trip through the URL, a reload and the nav', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  const w = 'cpu:1,disk:3,mem:2,net:0.5,pg:3,web:0';
  await openSummary(page, `?view=summary&${sidesQuery(cmp)}&profile=custom&w=${encodeURIComponent(w)}`);
  expect(params(page).get('profile')).toBe('custom');
  expect(params(page).get('w')).toBe(w);
  await page.reload();
  await expect(page.locator('#view-summary')).toBeVisible();
  expect(params(page).get('profile')).toBe('custom');
  expect(params(page).get('w')).toBe(w);

  // the nav keeps them, so a link to another view and back shows the same chart
  await page.locator('nav a[data-view="compare"]').click();
  await expect(page.locator('#view-compare')).toBeVisible();
  expect(params(page).get('profile')).toBe('custom');
  expect(params(page).get('w')).toBe(w);
  await page.locator('nav a[data-view="summary"]').click();
  await expect(page.locator('#view-summary')).toBeVisible();
  expect(params(page).get('w')).toBe(w);
  expect(params(page).getAll('other')).toEqual(cmp.others);

  // a named profile: the default is not written to the URL, another one is
  await page.goto(`?view=summary&${sidesQuery(cmp)}&profile=db`);
  await expect(page.locator('#view-summary')).toBeVisible();
  expect(params(page).get('profile')).toBe('db');
  await page.goto(`?view=summary&${sidesQuery(cmp)}`);
  expect(params(page).get('profile')).toBeNull();
});

test('summary: without compared sides it asks for them, and an invalid w does not break the view', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  await openView(page, `?view=compare&${sidesQuery(cmp)}`);
  await page.goto(`?view=summary&base=${encodeURIComponent(cmp.base)}`);
  await expect(page.locator('#view-summary')).toBeVisible();
  await expect(page.locator('#sum-status .state.empty')).toBeVisible();
  await expect(page.locator('#sum-body')).toBeHidden();

  await openSummary(page, `?view=summary&${sidesQuery(cmp)}&profile=custom&w=nonsense`);
  expect(params(page).get('w')).toBe('nonsense');
});

for (const width of [1280, 400]) {
  test(`summary: no sideways scroll at ${width}px`, async ({ browser, request }) => {
    const [cmp] = await pickComparisons(request);
    const ctx = await browser.newContext({ viewport: { width, height: 900 } });
    const page = await ctx.newPage();
    const errors = [];
    page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    page.on('pageerror', (e) => errors.push(e.message));
    try {
      await openSummary(page, `?view=summary&${sidesQuery(cmp)}`);
      const o = await sidewaysOverflow(page);
      expect(o.scroll).toBe(0);
      expect(o.clipped).toEqual([]);
      expect(errors).toEqual([]);
    } finally {
      await ctx.close();
    }
  });
}
