// The numbers on screen are the API's: for two comparisons (groups across architectures, runs of one group)
// the tally table, the takeaway, every heatmap cell (verdict and percentage) and the dot plot.

import { test, expect, api, pickComparisons, sidesQuery, openView, pct, VERDICT_TEXT } from './helpers.js';

let comparisons = [];
test.beforeAll(async ({ request }) => { comparisons = await pickComparisons(request); });

for (const i of [0, 1]) {
  test.describe(`comparison ${i + 1}`, () => {
    test('tally table and takeaway equal /api/compare', async ({ page, request }) => {
      const cmp = comparisons[i];
      test.info().annotations.push({ type: 'comparison', description: `${cmp.name}: ${sidesQuery(cmp)}` });
      const c = await api(request, 'api/compare?' + sidesQuery(cmp));
      await openView(page, '?' + sidesQuery(cmp));

      const rows = await page.$$eval('#tally tbody tr', (trs) => trs.map((tr) => ({
        cells: [...tr.children].slice(0, 4).map((td) => td.textContent),
        sections: [...tr.querySelectorAll('td.sec')].map((td) => td.getAttribute('aria-label') || td.textContent),
      })));
      expect(rows).toHaveLength(c.tallies.length);
      const sectionCount = new Set(c.tallies.flatMap((t) => Object.keys(t.by_section))).size;
      c.tallies.forEach((t, k) => {
        expect(rows[k].cells, `tally row ${k}`).toEqual([c.others[k].label, String(t.better), String(t.worse), String(t.not_clear)]);
        expect(rows[k].sections).toHaveLength(sectionCount);
        const want = Object.values(t.by_section).map((v) => `${v[0]} better, ${v[1]} worse, ${v[2]} within noise`);
        expect(rows[k].sections.filter((x) => x !== '–').sort(), `tally row ${k} by section`).toEqual(want.sort());
      });

      const nums = await page.locator('#takeaway .num').allTextContents();
      expect(nums, 'takeaway').toEqual([c.tallies[0].better, c.tallies[0].worse, c.tallies[0].not_clear].map(String));
      await expect(page.locator('#warnings li')).toHaveText(c.warnings);
    });

    test('every heatmap cell: verdict and percentage equal /api/compare', async ({ page, request }) => {
      const cmp = comparisons[i];
      const c = await api(request, 'api/compare?' + sidesQuery(cmp));
      await openView(page, '?' + sidesQuery(cmp) + '&section=all');

      const heat = await page.$$eval('#heat tbody tr', (trs) => trs.filter((tr) => tr.querySelector('th button')).map((tr) => ({
        title: tr.querySelector('th button').title,
        cells: [...tr.querySelectorAll('td .cell')].map((s) => ({
          cls: s.className.replace(/^cell /, ''),
          pct: s.childNodes[0] && s.childNodes[0].nodeType === Node.TEXT_NODE ? s.childNodes[0].textContent : s.textContent,
          sr: s.querySelector('.sr-only') ? s.querySelector('.sr-only').textContent.trim() : '',
        })),
      })));

      // one heatmap row per metric id (rows are identified by "name (unit)")
      const ids = [...new Set(c.rows.map((r) => r.id))];
      const keyOf = (r) => `${r.name} (${r.unit}) · show every value`;
      const keyById = new Map(c.rows.map((r) => [r.id, keyOf(r)]));
      expect(new Set(keyById.values()).size, 'metric names are unique, so rows can be matched by name').toBe(ids.length);
      expect(heat.map((r) => r.title).sort(), 'heatmap rows').toEqual(ids.map((id) => keyById.get(id)).sort());
      const byTitle = new Map(heat.map((r) => [r.title, r.cells]));

      const mismatches = [];
      for (const id of ids) {
        const cells = byTitle.get(keyById.get(id));
        if (cells.length !== c.others.length) { mismatches.push(`${id}: ${cells.length} cells`); continue; }
        c.others.forEach((_, k) => {
          const r = c.rows.find((x) => x.id === id && x.other === k);
          const got = cells[k];
          if (!r) { if (got.cls !== 'missing') mismatches.push(`${id} side ${k}: want missing, got ${got.cls}`); return; }
          const cls = r.cross_arch ? 'cross' : r.verdict;
          const once = Math.min(r.n_base, r.n_value) < 2;
          const text = pct(r.diff) + (once ? '·' : '');
          const sr = r.cross_arch ? 'not counted' : VERDICT_TEXT[r.verdict];
          if (got.cls !== cls || got.pct !== text || got.sr !== sr) {
            mismatches.push(`${id} side ${k}: want ${cls} "${text}" "${sr}", got ${got.cls} "${got.pct}" "${got.sr}" (diff ${r.diff})`);
          }
        });
      }
      expect(mismatches, `${c.rows.length} heatmap cells`).toEqual([]);
    });

    test('dot plot: one dot per value of /api/values, stats from /api/compare', async ({ page, request }) => {
      const cmp = comparisons[i];
      const c = await api(request, 'api/compare?' + sidesQuery(cmp));
      // the metric with the most values, and a cross-architecture one when there is one
      const byN = [...c.rows].sort((a, b) => (b.n_base + b.n_value) - (a.n_base + a.n_value) || a.id.localeCompare(b.id));
      const picks = [...new Set([byN[0].id, ...(c.rows.find((r) => r.cross_arch) ? [c.rows.find((r) => r.cross_arch).id] : []), byN.at(-1).id])];
      const sides = [c.base, ...c.others];
      for (const id of picks) {
        const vals = await Promise.all(sides.map((s) => api(request, `api/values?ref=${encodeURIComponent(s.reference)}&id=${encodeURIComponent(id)}`)));
        const counts = vals.map((v) => v.values.length);
        await openView(page, `?view=metric&${sidesQuery(cmp)}&metric=${encodeURIComponent(id)}`);
        await expect(page.locator('#metric')).toHaveValue(id);
        await expect(page.locator('#dots svg circle'), `${id}: dots`).toHaveCount(counts.reduce((a, b) => a + b, 0));
        // per side: each side's dots are drawn in their own group, in side order
        const perSide = await page.$$eval('#dots svg > g', (gs) => gs.map((g) => g.querySelectorAll(':scope > circle').length).filter((n) => n > 0));
        expect(perSide, `${id}: dots per side`).toEqual(counts.filter((n) => n > 0));
        const label = await page.locator('#dots svg').getAttribute('aria-label');
        expect(label).toContain(`Dot plot of ${counts.reduce((a, b) => a + b, 0)} values`);

        // statistics table: one row for the baseline, one per compared side, counts and verdicts as the API says
        const rows = c.rows.filter((r) => r.id === id);
        const stats = await page.$$eval('#metric-stats tbody tr', (trs) => trs.map((tr) => [...tr.children].map((x) => x.textContent)));
        expect(stats).toHaveLength(1 + rows.length);
        expect(stats[0][3], `${id}: values of the baseline`).toBe(String(rows[0].n_base));
        rows.forEach((r, k) => {
          expect(stats[k + 1][0]).toBe(c.others[r.other].label);
          expect(stats[k + 1][3], `${id}: values of side ${r.other}`).toBe(String(r.n_value));
          expect(stats[k + 1][4], `${id}: difference`).toBe(pct(r.diff));
          expect(stats[k + 1][6], `${id}: verdict`).toBe(r.cross_arch ? 'not counted (sysbench across architectures)' : VERDICT_TEXT[r.verdict]);
        });
      }
    });
  });
}

// F1 (K): a group whose runs carry two `round` tags gets a quick pick that pools each round
// (group:NAME@round=VALUE); its tally is the API's and the picker names the subset sides.
test('round quick pick compares pooled rounds (group:NAME@round=VALUE)', async ({ page, request }) => {
  await openView(page, '?view=compare');
  const pick = page.locator('#presets button', { hasText: /^Round / }).first();
  await expect(pick, 'the POC registry gives x86-high two rounds').toHaveCount(1);
  const base = await pick.getAttribute('data-base');
  const others = JSON.parse(await pick.getAttribute('data-others'));
  expect(base).toMatch(/^group:[^@]+@round=.+$/);
  await pick.click();
  await expect(page.locator('#heat tbody tr').first()).toBeVisible();
  const c = await api(request, 'api/compare?' + sidesQuery({ base, others }));
  expect(c.base.label).toMatch(/ · round=/);
  const row = await page.$$eval('#tally tbody tr', (trs) => [...trs[0].children].slice(0, 4).map((td) => td.textContent));
  expect(row).toEqual([c.others[0].label, String(c.tallies[0].better), String(c.tallies[0].worse), String(c.tallies[0].not_clear)]);
  await expect(page.locator('#base option:checked')).toContainText(c.base.label);
});
