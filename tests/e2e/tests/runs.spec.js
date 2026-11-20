// Runs view behaviour beyond tag saving (tags.spec.js): an open tag form keeps its draft and the caret across
// re-renders, report.md is linked exactly for the runs that kept one, and the delete dialog says what deleting
// does and changes nothing on Cancel. scripts/e2e.sh writes a placeholder report.md into every fixture copy but
// BENCHLENS_NO_REPORT.

import { test, expect, api, openView } from './helpers.js';

const NO_REPORT = process.env.BENCHLENS_NO_REPORT || 'x86-std-r2-20261119-103253';
const row = (page, id) => page.locator(`#runs-table tr[data-run="${id}"]`);

test('runs: a tag draft survives re-renders, a ticked checkbox and a filter change', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  const [a, b] = runs;
  await openView(page, '?view=runs');
  await row(page, a.run_id).getByRole('button', { name: 'Edit tags of ' + a.label }).click();
  const form = row(page, a.run_id).locator('form.tag-form');
  await form.getByRole('button', { name: '+ tag' }).click();
  await form.locator('.row').last().getByLabel('Tag name').fill('e2e_draft');
  await form.locator('.row').last().getByLabel('Tag value').click();
  await page.keyboard.type('draft-val');
  const draft = async () => {
    const last = row(page, a.run_id).locator('form.tag-form .row').last();
    return [await last.getByLabel('Tag name').inputValue(), await last.getByLabel('Tag value').inputValue()];
  };

  // a re-render while typing (what a reload after another write does): draft, focus and caret stay
  await page.evaluate(() => window.dispatchEvent(new Event('statechange')));
  expect(await draft()).toEqual(['e2e_draft', 'draft-val']);
  const caret = await page.evaluate(() => ({ tag: document.activeElement?.getAttribute('aria-label'), at: document.activeElement?.selectionStart }));
  expect(caret).toEqual({ tag: 'Tag value', at: 'draft-val'.length });

  await row(page, b.run_id).getByRole('checkbox').check();
  expect(await draft()).toEqual(['e2e_draft', 'draft-val']);
  await expect(row(page, b.run_id).getByRole('checkbox')).toBeChecked();

  await page.fill('#runs-q', a.label);
  await page.fill('#runs-q', '');
  expect(await draft()).toEqual(['e2e_draft', 'draft-val']);

  await row(page, a.run_id).locator('form.tag-form').getByRole('button', { name: 'Cancel' }).click();
  await expect(row(page, a.run_id).locator('form.tag-form')).toHaveCount(0);
  await expect(row(page, a.run_id).getByRole('button', { name: 'Edit tags of ' + a.label })).toBeFocused();
  // nothing was saved
  expect((await api(request, 'api/runs')).find((r) => r.run_id === a.run_id).tags).toEqual(a.tags);
});

test('runs: report.md is linked only for runs that kept it', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  await openView(page, '?view=runs');
  let kept = 0, missing = 0;
  for (const r of runs) {
    const d = await api(request, 'api/runs/' + r.run_id);
    const has = d.files.some((f) => f.name === 'report.md');
    if (has) kept++; else missing++;
    if (r.folder === NO_REPORT) expect(has, `${NO_REPORT} keeps no report.md`).toBe(false);
    const tr = row(page, r.run_id);
    await tr.scrollIntoViewIfNeeded();
    // the link appears once the row's file list is known (fetched when the row scrolls into view)
    await expect(tr.locator('.pending-report')).toHaveCount(0);
    await expect(tr.locator('.report-slot a[href]', { hasText: 'report.md' })).toHaveCount(has ? 1 : 0);
    if (has) {
      const href = await tr.locator('.report-slot a').getAttribute('href');
      const res = await request.get(href);
      expect(res.status(), href).toBe(200);
    } else {
      await expect(tr.locator('.report-slot .no-report')).toHaveText('no report.md');
    }
  }
  expect(kept, 'runs with report.md').toBeGreaterThan(0);
  expect(missing, 'runs without report.md').toBeGreaterThan(0);
});

test('runs: the delete dialog says tags are not restored; Cancel keeps the run', async ({ page, request }) => {
  const runs = await api(request, 'api/runs');
  const r = runs[0];
  await openView(page, '?view=runs');
  await row(page, r.run_id).getByRole('button', { name: 'Delete ' + r.label }).click();
  const dlg = page.locator('#confirm-dialog');
  await expect(dlg).toBeVisible();
  await expect(dlg).toContainText('removes the run and its values');
  await expect(dlg).toContainText('Importing the same folder again brings the values back');
  await expect(dlg).toContainText('tags edited here or given by a registry are not restored unless they are given again');
  await dlg.getByRole('button', { name: 'Cancel' }).click();
  await expect(dlg).toBeHidden();
  await expect(row(page, r.run_id)).toHaveCount(1);
  expect((await api(request, 'api/runs')).length).toBe(runs.length);
});
