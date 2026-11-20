// Tag editing in the Runs view writes through PATCH /api/runs/{id}/tags: the change shows in /api/runs.
// Runs against the temporary database of scripts/e2e.sh; the test puts the run's tags back at the end.

import { test, expect, api, openView } from './helpers.js';

test('add, change and remove a tag in the UI; /api/runs follows', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  const run = runs[runs.length - 1];
  const original = { ...run.tags };
  const key = 'e2e-check';
  const value = 'set from the UI ' + Date.now();
  const tagsOf = async () => (await api(request, 'api/runs')).find((r) => r.run_id === run.run_id).tags;
  const row = page.locator(`#runs-table tr[data-run="${run.run_id}"]`);

  try {
    await openView(page, '?view=runs');

    // add
    await row.getByRole('button', { name: 'Edit tags of ' + run.label }).click();
    const form = row.locator('form.tag-form');
    await expect(form.locator('.row')).toHaveCount(Object.keys(original).length);
    await form.getByRole('button', { name: '+ tag' }).click();
    const last = form.locator('.row').last();
    await last.getByLabel('Tag name').fill(key);
    await last.getByLabel('Tag value').fill(value);
    const patch = page.waitForResponse((r) => r.request().method() === 'PATCH' && r.url().includes(`/api/runs/${run.run_id}/tags`));
    await form.getByRole('button', { name: 'Save' }).click();
    expect((await patch).status()).toBe(200);
    await expect(row.locator('form.tag-form')).toHaveCount(0);
    await expect(row.locator('.chip', { hasText: key })).toHaveAttribute('title', `${key}=${value}`);
    expect(await tagsOf()).toEqual({ ...original, [key]: value });

    // the new tag is a filter value and filters in /api/runs too
    await expect(page.locator(`#runs-tag option[value="${key}:${value}"]`)).toHaveCount(1);
    const filtered = await api(request, 'api/runs?tag=' + encodeURIComponent(`${key}:${value}`));
    expect(filtered.map((r) => r.run_id)).toEqual([run.run_id]);

    // a reload shows what the server has
    await page.reload();
    await openView(page, '?view=runs');
    await expect(row.locator('.chip', { hasText: key })).toHaveAttribute('title', `${key}=${value}`);

    // remove it with the × button
    await row.getByRole('button', { name: 'Edit tags of ' + run.label }).click();
    await row.getByRole('button', { name: 'Remove tag ' + key }).click();
    await row.locator('form.tag-form').getByRole('button', { name: 'Save' }).click();
    await expect(row.locator('form.tag-form')).toHaveCount(0);
    await expect(row.locator('.chip', { hasText: key })).toHaveCount(0);
    expect(await tagsOf()).toEqual(original);

    // a bad tag name is refused in the browser and nothing is sent
    await row.getByRole('button', { name: 'Edit tags of ' + run.label }).click();
    await row.getByRole('button', { name: '+ tag' }).click();
    await row.locator('form.tag-form .row').last().getByLabel('Tag name').fill('bad key!');
    await row.locator('form.tag-form .row').last().getByLabel('Tag value').fill('x');
    await row.locator('form.tag-form').getByRole('button', { name: 'Save' }).click();
    await expect(row.locator('form.tag-form .error')).toBeVisible();
    await row.locator('form.tag-form').getByRole('button', { name: 'Cancel' }).click();
    expect(await tagsOf()).toEqual(original);
  } finally {
    // leave the database as it was for the other tests
    const now = await tagsOf();
    const restore = {};
    for (const k of Object.keys(now)) if (!(k in original)) restore[k] = '';
    for (const [k, v] of Object.entries(original)) if (now[k] !== v) restore[k] = v;
    if (Object.keys(restore).length) await request.patch(`api/runs/${run.run_id}/tags`, { data: restore });
  }
});
