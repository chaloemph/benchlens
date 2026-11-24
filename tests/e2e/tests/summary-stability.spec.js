// Summary block "Stability": numbers equal score.js for the same comparison, sides measured once are listed
// and not drawn, the table fallback has the same numbers, no sideways scroll at 400 px.

import { test, expect, api, pickComparisons, sidesQuery, sidewaysOverflow } from './helpers.js';
import { scoresFor, weightsFor, SECTIONS } from '../../../web/js/score.js';

const NAMES = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', pg: 'PostgreSQL', net: 'Network', web: 'Web' };
const f1 = (v) => v.toFixed(1) + '%';
const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

async function open(page, cmp, width) {
  if (width) await page.setViewportSize({ width, height: 900 });
  await page.goto(`?view=summary&${sidesQuery(cmp)}`);
  await expect(page.locator('#sum-body')).toBeVisible();
  await expect(page.locator('#sum-stability h2')).toHaveText('Stability');
}

const comparisonOf = (request, cmp) => api(request, 'api/compare?' + sidesQuery(cmp));

/** The table fallback as { header: [...], rows: { section: [cells] } }. */
const readTable = (page) => page.locator('#sum-stability details.sum-table table').evaluate((t) => ({
  header: [...t.tHead.rows[0].cells].map((c) => c.textContent),
  rows: Object.fromEntries([...t.tBodies[0].rows].map((r) => [r.cells[0].textContent, [...r.cells].slice(1).map((c) => c.textContent)])),
}));

test('stability: numbers equal score.js, in the table and in the dots', async ({ page, request }) => {
  const cmps = await pickComparisons(request);
  for (const cmp of cmps) {
    const comparison = await comparisonOf(request, cmp);
    const scores = scoresFor(comparison, weightsFor('balanced', '').weights);
    const series = [scores.base, ...scores.sides];
    await open(page, cmp);
    const table = await readTable(page);
    expect(table.header).toHaveLength(2 + series.length);
    let drawnDots = 0;
    for (const sec of SECTIONS) {
      const cells = table.rows[NAMES[sec]];
      expect(cells, `row ${sec}`).toBeTruthy();
      series.forEach((s, i) => {
        const cell = cells[1 + i];
        const allNull = !SECTIONS.some((x) => isNum(s.stability[x]));
        if (allNull) expect(cell).toBe('measured once');
        else if (isNum(s.stability[sec])) {
          expect(cell.startsWith(f1(s.stability[sec])), `${cmp.name} ${sec} ${s.label}: ${cell}`).toBe(true);
          drawnDots++;
        } else expect(cell).toBe('no data');
      });
    }
    expect(await page.locator('#sum-stability .sum-stab-wrap path.sum-stab-dot').count()).toBe(drawnDots);

    // sides measured once are listed and not drawn at 0
    const once = series.filter((s) => !SECTIONS.some((x) => isNum(s.stability[x])));
    await expect(page.locator('#sum-stability .sum-stab-once li')).toHaveCount(once.length);
    for (const s of once) await expect(page.locator('#sum-stability .sum-stab-once')).toContainText(`${s.label}`);
    // the sentence names the noisiest side and its value
    const lead = page.locator('#sum-stability .sum-stab-lead');
    if (drawnDots) {
      let top = null;
      for (const s of series) for (const sec of SECTIONS) if (isNum(s.stability[sec]) && (!top || s.stability[sec] > top.v)) top = { s, v: s.stability[sec] };
      await expect(lead).toContainText(top.s.label);
      await expect(lead).toContainText(f1(top.v));
    } else {
      await expect(lead).toContainText('no stability data');
    }
  }
});

test('stability: dot tooltip shows the verdict counts behind the section', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  const comparison = await comparisonOf(request, cmp);
  const scores = scoresFor(comparison, weightsFor('balanced', '').weights);
  await open(page, cmp);
  const side = scores.sides[0];
  const sec = SECTIONS.find((x) => isNum(side.stability[x]));
  test.skip(!sec, 'the fixture side was measured once');
  const label = `${side.label}: ${NAMES[sec]}, median CV ${f1(side.stability[sec])}`;
  const c = side.sections[sec].counts;
  const labels = await page.locator('#sum-stability .sum-stab-wrap path.sum-stab-dot').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
  const mine = labels.filter((l) => l.startsWith(label));
  expect(mine).toHaveLength(1);
  expect(mine[0]).toContain(`better ${c.better} · worse ${c.worse} · within noise ${c.not_clear}`);
});

test('stability: 400 px wide has no sideways scroll, the legend and the table stay reachable', async ({ page, request }) => {
  const [cmp] = await pickComparisons(request);
  await open(page, cmp, 400);
  const o = await sidewaysOverflow(page);
  expect(o.scroll).toBe(0);
  expect(o.clipped).toEqual([]);
  await page.locator('#sum-stability summary').click();
  await expect(page.locator('#sum-stability table.data')).toBeVisible();
});
