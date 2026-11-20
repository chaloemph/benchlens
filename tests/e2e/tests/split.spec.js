// Compare picker: split a group by tag. The group, tag and values are chosen from /api/runs (nothing hard-coded);
// each chosen value becomes a side group:NAME@KEY=VALUE, shown with the API's label, in the URL, and can be
// dropped again. Driven with the keyboard; at 400 px nothing goes past the right edge.

import { test, expect, api, openView, sidewaysOverflow } from './helpers.js';

const params = (page) => new URL(page.url()).searchParams;

/** A group whose runs carry a tag (not `group`) with at least two values; `round` first when there is one. */
async function pickSplit(request) {
  const runs = await api(request, 'api/runs');
  const found = [];
  for (const group of [...new Set(runs.map((r) => r.group))].sort()) {
    if (group.includes('@')) continue;
    const mine = runs.filter((r) => r.group === group);
    const keys = [...new Set(mine.flatMap((r) => Object.keys(r.tags)))].filter((k) => k !== 'group').sort();
    for (const key of keys) {
      const values = [...new Set(mine.map((r) => r.tags[key]).filter((v) => v && !v.includes('@')))]
        .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
      if (values.length >= 2) found.push({ group, key, values });
    }
  }
  expect(found.length, 'a group with a tag of two values').toBeGreaterThan(0);
  return found.find((f) => f.key === 'round') || found[0];
}

const ref = (sp, v) => `group:${sp.group}@${sp.key}=${v}`;

async function chooseSplit(page, sp) {
  const box = page.locator('#pick-split');
  await box.locator('summary').focus();
  await page.keyboard.press('Enter');
  await expect(box).toHaveAttribute('open', '');
  await page.locator('#split-group').focus();
  await page.locator('#split-group').selectOption(sp.group);
  await page.locator('#split-key').selectOption(sp.key);
  await expect(page.locator('#split-values input')).toHaveCount(sp.values.length);
  await expect(page.locator('#split-add')).toBeDisabled();
  for (const v of sp.values) {
    await page.locator(`#split-values input[value="${v}"]`).focus();
    await page.keyboard.press('Space');
  }
  await expect(page.locator('#split-add')).toHaveText(`Add ${sp.values.length} sides`);
}

test('compare: split a group by tag adds one side per value, labelled, and they can be dropped', async ({ page, request }) => {
  const sp = await pickSplit(request);
  const groups = await api(request, 'api/groups');
  const base = 'group:' + groups.map((g) => g.group).find((g) => g !== sp.group);
  // a baseline alone: nothing compared yet
  await page.goto('?base=' + encodeURIComponent(base));
  await expect(page.locator('#compare-status .state.empty')).toBeVisible();
  await chooseSplit(page, sp);
  await page.locator('#split-add').focus();
  await page.keyboard.press('Enter');

  const refs = sp.values.map((v) => ref(sp, v));
  await expect.poll(() => params(page).getAll('other')).toEqual(refs);
  expect(params(page).get('base')).toBe(base);
  await expect(page.locator('#split-msg')).toHaveText(`${refs.length} sides added.`);
  const c = await api(request, 'api/compare?' + new URLSearchParams([['base', base], ...refs.map((r) => ['other', r])]));
  expect(c.others.map((o) => o.label)).toEqual(sp.values.map((v) => `${sp.group} · ${sp.key}=${v}`));
  await expect(page.locator('#tally tbody tr th')).toHaveText(c.others.map((o) => o.label));

  // listed under the control with readable labels, ticked; unticking drops that side
  const picked = page.locator('#split-current input');
  await expect(picked).toHaveCount(refs.length);
  for (const r of refs) await expect(page.locator(`#split-current input[value="${r}"]`)).toBeChecked();
  await expect(page.locator('#split-current label').first()).toContainText(`${sp.group} · ${sp.key}=${sp.values[0]}`);
  // click, not uncheck(): the list is redrawn at once, so the checkbox clicked is gone afterwards
  await page.locator(`#split-current input[value="${refs[0]}"]`).click();
  await expect(page.locator(`#split-current input[value="${refs[0]}"]`)).toHaveCount(0);
  await expect.poll(() => params(page).getAll('other')).toEqual(refs.slice(1));
  await expect(page.locator('#tally tbody tr')).toHaveCount(refs.length - 1);

  // ticking a listed group keeps the split sides after it
  const extra = groups.map((g) => 'group:' + g.group).find((g) => g !== base);
  await page.locator(`#pick-groups input[value="${extra}"]`).check();
  await expect.poll(() => params(page).getAll('other')).toEqual([extra, ...refs.slice(1)]);
});

test('compare: split with the first value as the baseline; at 400 px nothing is cut off', async ({ page, request }) => {
  const sp = await pickSplit(request);
  await page.setViewportSize({ width: 400, height: 900 });
  await openView(page, '?');
  await chooseSplit(page, sp);
  await page.locator('#split-as-base').check();
  await page.locator('#split-add').click();
  const refs = sp.values.map((v) => ref(sp, v));
  await expect.poll(() => params(page).get('base')).toBe(refs[0]);
  expect(params(page).getAll('other')).toContain(refs[1]);
  expect(params(page).getAll('other')).not.toContain(refs[0]);
  await expect(page.locator('#base')).toHaveValue(refs[0]);
  await expect(page.locator('#base option:checked')).toContainText(`${sp.group} · ${sp.key}=${sp.values[0]}`);
  await expect(page.locator('#heat tbody tr').first()).toBeVisible();
  const o = await sidewaysOverflow(page);
  expect(o.scroll, 'sideways scroll').toBeLessThanOrEqual(0);
  expect(o.clipped).toEqual([]);
});
