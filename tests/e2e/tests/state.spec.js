// URL state: a selection made in the UI is in the URL, and a reload (or the link alone) restores it.

import { test, expect, api, pickComparisons, sidesQuery, openView, waitForView } from './helpers.js';

const params = (page) => new URL(page.url()).searchParams;

test('compare: baseline, compared sides and section survive a reload', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  const groups = await api(request, 'api/groups');
  const extra = groups.map((g) => 'group:' + g.group).find((ref) => ref !== cmp.base && !cmp.others.includes(ref));
  await openView(page, '?view=compare&' + sidesQuery(cmp));

  // pick through the UI: one more group to compare with, then the "all" section
  if (extra) {
    await page.locator(`#pick-groups input[value="${extra}"]`).check();
    await expect(page.locator('#tally tbody tr')).toHaveCount(cmp.others.length + 1);
  }
  await page.locator('#section').selectOption('all');
  const others = params(page).getAll('other');
  expect(params(page).get('base')).toBe(cmp.base);
  expect(others.sort()).toEqual([...cmp.others, ...(extra ? [extra] : [])].sort());
  expect(params(page).get('section')).toBe('all');
  const before = await page.locator('#tally tbody tr th').allTextContents();
  const heatRows = await page.locator('#heat tbody tr').count();

  await page.reload();
  await waitForView(page, 'compare');
  await expect(page.locator('#base')).toHaveValue(cmp.base);
  await expect(page.locator('#section')).toHaveValue('all');
  for (const ref of others) await expect(page.locator(`#pick-groups input[value="${ref}"], #pick-runs input[value="${ref}"]`)).toBeChecked();
  await expect(page.locator('#tally tbody tr th')).toHaveText(before);
  await expect(page.locator('#heat tbody tr')).toHaveCount(heatRows);

  // a new baseline from the picker drops it from the compared sides
  await page.locator('#base').selectOption(others[0]);
  expect(params(page).get('base')).toBe(others[0]);
  expect(params(page).getAll('other')).not.toContain(others[0]);
  await page.reload();
  await waitForView(page, 'compare');
  await expect(page.locator('#base')).toHaveValue(others[0]);

  // back goes to the previous selection
  await page.goBack();
  await waitForView(page, 'compare');
  await expect(page.locator('#base')).toHaveValue(cmp.base);
});

test('compare: single runs picked in the UI survive a reload', async ({ page, request }) => {
  const [, runsCmp] = await pickComparisons(request);
  await openView(page, `?view=compare&base=${encodeURIComponent(runsCmp.base)}&other=${encodeURIComponent(runsCmp.others[0])}`);
  await expect(page.locator('#view-compare details.picks-runs')).toHaveAttribute('open', '');
  await expect(page.locator(`#pick-runs input[value="${runsCmp.others[0]}"]`)).toBeChecked();
  await page.locator(`#pick-runs input[value="${runsCmp.others[0]}"]`).uncheck();
  await expect(page.locator('#compare-status .state.empty')).toBeVisible();
  expect(params(page).getAll('other')).toEqual([]);
  await page.reload();
  await expect(page.locator('#compare-status .state.empty')).toBeVisible();
  await expect(page.locator('#base')).toHaveValue(runsCmp.base);
});

test('metric: the chosen metric survives a reload; heatmap name opens it', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  const c = await api(request, 'api/compare?' + sidesQuery(cmp));
  await openView(page, '?view=compare&' + sidesQuery(cmp) + '&section=all');
  const row = c.rows[c.rows.length - 1];
  await page.locator('#heat tbody th button', { hasText: row.name }).first().click();
  await waitForView(page, 'metric');
  expect(params(page).get('view')).toBe('metric');
  expect(params(page).get('metric')).toBe(row.id);
  await expect(page.locator('#metric')).toHaveValue(row.id);

  const other = c.rows.find((r) => r.id !== row.id);
  await page.locator('#metric').selectOption(other.id);
  await expect(page.locator('#metric-desc')).toContainText('id ' + other.id);
  expect(params(page).get('metric')).toBe(other.id);
  await page.reload();
  await waitForView(page, 'metric');
  await expect(page.locator('#metric')).toHaveValue(other.id);
  await expect(page.locator('#metric-desc')).toContainText('id ' + other.id);

  await page.click('#back-compare');
  await waitForView(page, 'compare');
  expect(params(page).get('base')).toBe(cmp.base);
});

test('runs: filters survive a reload; hardware of selected runs is a link too', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  const r0 = runs[0];
  await openView(page, '?view=runs');
  await page.locator('#runs-arch').selectOption(r0.arch);
  await page.locator('#runs-group').selectOption(r0.group);
  const want = runs.filter((r) => r.arch === r0.arch && r.group === r0.group);
  await expect(page.locator('#runs-table tbody tr[data-run]')).toHaveCount(want.length);
  expect(params(page).get('arch')).toBe(r0.arch);
  expect(params(page).get('group')).toBe(r0.group);
  await page.reload();
  await waitForView(page, 'runs');
  await expect(page.locator('#runs-arch')).toHaveValue(r0.arch);
  await expect(page.locator('#runs-group')).toHaveValue(r0.group);
  await expect(page.locator('#runs-table tbody tr[data-run]')).toHaveCount(want.length);

  await page.locator('#runs-q').fill(r0.folder);
  await expect(page.locator('#runs-table tbody tr[data-run]')).toHaveCount(want.filter((r) => r.folder.includes(r0.folder)).length);
  await page.reload();
  await waitForView(page, 'runs');
  await expect(page.locator('#runs-q')).toHaveValue(r0.folder);

  await page.locator(`#runs-table tr[data-run="${r0.run_id}"] input[type=checkbox]`).check();
  await page.click('#runs-hw');
  await expect(page.locator('#view-hardware')).toBeVisible();
  expect(params(page).getAll('run')).toEqual([r0.run_id]);
  await expect(page.locator('#hw-table thead th')).toHaveCount(2);
  await page.reload();
  await expect(page.locator('#hw-table thead th')).toHaveCount(2);
  await expect(page.locator('#hw-table thead th').nth(1)).toContainText(r0.label);
});
