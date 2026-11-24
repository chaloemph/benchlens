// Summary view, Radar block: the points and the table fallback carry the numbers score.js returns for the same
// comparison (computed here with the same module), axes stay in the fixed order, a section without data is a gap
// (no marker, "no data" on the label), and the block works in the dark theme and at 400 px.

import { test, expect, api, pickComparisons, sidesQuery, sidewaysOverflow } from './helpers.js';
import { scoresFor, weightsFor, SECTIONS } from '../../../web/js/score.js';

const SLOT = '#sum-radar';
const NAMES = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', pg: 'PostgreSQL', net: 'Network', web: 'Web' };

let comparisons = [];
test.beforeAll(async ({ request }) => { comparisons = await pickComparisons(request); });

async function openRadar(page, query) {
  await page.goto('?view=summary&' + query);
  await expect(page.locator('#sum-body')).toBeVisible();
  await expect(page.locator(`${SLOT} svg.sum-radar-svg`)).toBeVisible();
}

/** What score.js says for the sides of a comparison (balanced profile, the URL default). */
async function expectedScores(request, cmp) {
  const comparison = await api(request, 'api/compare?' + sidesQuery(cmp));
  return scoresFor(comparison, weightsFor('balanced', '').weights);
}

for (const i of [0, 1]) {
  test(`radar ${i + 1}: points and table equal score.js, axes in fixed order`, async ({ page, request }) => {
    const cmp = comparisons[i];
    test.info().annotations.push({ type: 'comparison', description: `${cmp.name}: ${sidesQuery(cmp)}` });
    const scores = await expectedScores(request, cmp);
    await openRadar(page, sidesQuery(cmp));

    // axis labels: fixed order, "Name (n)" or "Name (n)" plus a note
    const labels = await page.locator(`${SLOT} .sum-radar-label`).evaluateAll((els) => els.map((e) => e.firstChild.textContent));
    const want = SECTIONS.map((sec) => {
      const ns = scores.sides.map((s) => s.sections[sec].n);
      const lo = Math.min(...ns), hi = Math.max(...ns);
      return `${NAMES[sec]} (${lo === hi ? lo : lo + '–' + hi})`;
    });
    expect(labels).toEqual(want);

    const shown = scores.sides.slice(0, 4);
    for (const side of shown) {
      for (const sec of SECTIONS) {
        const cell = side.sections[sec];
        const marker = page.locator(`${SLOT} .sum-radar-marker[data-side="${side.label}"][data-section="${sec}"]`);
        if (cell.score == null) {
          await expect(marker, `${side.label} ${sec}: a gap, not a point`).toHaveCount(0);
        } else {
          await expect(marker).toHaveCount(1);
          expect(Number(await marker.getAttribute('data-score')), `${side.label} ${sec}`).toBeCloseTo(cell.score, 9);
        }
      }
    }
    // a section with no data on every drawn side says so on its label
    for (const [k, sec] of SECTIONS.entries()) {
      if (shown.every((s) => s.sections[sec].score == null)) {
        await expect(page.locator(`${SLOT} .sum-radar-label`).nth(k)).toContainText('no data');
      }
    }

    // table fallback: every side and section with the score to one decimal and the verdict counts
    await page.locator(`${SLOT} details.sum-table summary`).click();
    const rows = await page.$$eval(`${SLOT} details.sum-table tbody tr`, (trs) => trs.map((tr) => [...tr.children].map((c) => c.textContent)));
    const expectRows = scores.sides.flatMap((side) => SECTIONS.map((sec) => {
      const c = side.sections[sec];
      return [side.label, NAMES[sec], c.score == null ? 'no data' : c.score.toFixed(1), String(c.n), String(c.counts.better), String(c.counts.worse), String(c.counts.not_clear), c.weak && c.n ? 'few results' : ''];
    }));
    expect(rows).toEqual(expectRows);
  });
}

test('radar: the baseline ring and a legend with dash and marker per side (colour is not the only cue)', async ({ page }) => {
  const cmp = comparisons[0];
  await openRadar(page, sidesQuery(cmp));
  await expect(page.locator(`${SLOT} .sum-radar-legend`)).toContainText('baseline = 100');
  await expect(page.locator(`${SLOT} .sum-radar-legend svg`)).toHaveCount(1 + cmp.others.length);
  await expect(page.locator(`${SLOT}`)).toContainText('Score (summary)');
});

test('radar: tooltip shows the score and the verdict counts', async ({ page, request }) => {
  const cmp = comparisons[0];
  const scores = await expectedScores(request, cmp);
  await openRadar(page, sidesQuery(cmp));
  const sec = SECTIONS.find((s) => scores.sides[0].sections[s].score != null);
  const cell = scores.sides[0].sections[sec];
  const marker = page.locator(`${SLOT} .sum-radar-marker[data-section="${sec}"]`).first();
  await marker.focus();
  const tip = page.locator('#tip');
  await expect(tip).toBeVisible();
  await expect(tip).toContainText(cell.score.toFixed(1));
  await expect(tip).toContainText(`better ${cell.counts.better} · worse ${cell.counts.worse} · within noise ${cell.counts.not_clear}`);
});

test('radar: more than four sides draw four and offer a selector', async ({ page, request }) => {
  // runs as sides: the fixtures hold three groups but seven runs
  const runs = await api(request, 'api/runs');
  expect(runs.length, 'the fixtures have six runs or more').toBeGreaterThanOrEqual(6);
  const ids = runs.map((r) => r.run_id);
  const cmp = { base: ids[0], others: ids.slice(1, 6) };
  await openRadar(page, sidesQuery(cmp));
  const boxes = page.locator(`${SLOT} .sum-radar-pick input`);
  await expect(boxes).toHaveCount(5);
  await expect(page.locator(`${SLOT} .sum-radar-legend svg`)).toHaveCount(5); // baseline + 4
  await expect(boxes.nth(4)).toBeDisabled();
  await boxes.nth(0).uncheck();
  await expect(boxes.nth(4)).toBeEnabled();
  await boxes.nth(4).check();
  await expect(page.locator(`${SLOT} .sum-radar-legend svg`)).toHaveCount(5);
});

for (const [name, width, scheme] of [['dark theme', 1280, 'dark'], ['400 px wide', 400, 'light']]) {
  test(`radar: ${name}, no sideways scroll`, async ({ browser, request }) => {
    const cmp = comparisons[0];
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
    const page = await ctx.newPage();
    const errors = [];
    page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    page.on('pageerror', (e) => errors.push(e.message));
    try {
      await openRadar(page, sidesQuery(cmp));
      const box = await page.locator(`${SLOT} svg.sum-radar-svg`).boundingBox();
      expect(box.width).toBeLessThanOrEqual(width);
      const o = await sidewaysOverflow(page);
      expect(o.scroll).toBe(0);
      expect(o.clipped).toEqual([]);
      expect(errors).toEqual([]);
    } finally {
      await ctx.close();
    }
  });
}
