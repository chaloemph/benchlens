// Open a static HTML export (`benchlens export html`) from disk in headless Chromium with the network off, and
// check that what it shows equals `benchlens compare --json` for the same sides. Run by tests/export/run.sh,
// which builds the inputs; needs the `playwright` npm package and its chromium (not a project dependency).
//
//   node tests/export/offline.mjs REPORT.html EXPORTED.json [SWAPPED.json] [QUERY CLI.json]... [--hostile]
//
// EXPORTED.json: `benchlens compare BASE OTHER... --json` for the exported selection (checked on first load).
// SWAPPED.json: the comparison with the first compared side as the baseline and the old baseline as the only
// compared side; reached by using the baseline picker and the checkboxes like a reader would.
// QUERY CLI.json: open REPORT.html?QUERY (e.g. `base=RUN_ID&other=group:NAME`) and check against CLI.json.
// --hostile: the export holds labels such as `</script><img onerror=...>`; check nothing ran and the text shows
// as text.

import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const { chromium } = createRequire(import.meta.url)('playwright');

const args = process.argv.slice(2);
const hostile = args.includes('--hostile');
const [report, exportedJson, swappedJson, ...pairs] = args.filter((a) => a !== '--hostile');
if (!report || !exportedJson) {
  console.error('usage: node tests/export/offline.mjs REPORT.html EXPORTED.json [SWAPPED.json] [QUERY CLI.json]... [--hostile]');
  process.exit(2);
}
const fileUrl = pathToFileURL(path.resolve(report)).href;
const problems = [];
const requests = [];

const browser = await chromium.launch();
const ctx = await browser.newContext({ offline: true, viewport: { width: 1280, height: 900 } });
// nothing but the file itself may be loaded
await ctx.route('**/*', (route) => {
  const u = route.request().url();
  if (u.startsWith('file:') && u.split('?')[0] === fileUrl) return route.continue();
  requests.push(u);
  return route.abort();
});
const page = await ctx.newPage();
page.on('console', (m) => { if (m.type() === 'error') problems.push(`console: ${m.text()}`); });
page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
page.on('dialog', (d) => { problems.push(`dialog opened: ${d.message()}`); d.dismiss(); });

async function settle() {
  await page.waitForFunction(() => !document.querySelector('.state.loading'), null, { timeout: 15000 });
  await page.waitForTimeout(100);
  const err = await page.$$eval('.state.error', (els) => els.map((e) => e.textContent));
  if (err.length) problems.push(`error shown: ${err.join(' | ')}`);
}

/** The tally table and every heatmap cell against a CLI Comparison (as web/dev/smoke.mjs does). */
async function checkAgainst(what, cliPath) {
  const c = JSON.parse(fs.readFileSync(cliPath, 'utf8'));
  await page.selectOption('#section', 'all');
  await settle();
  const tally = await page.$$eval('#tally tbody tr', (trs) => trs.map((tr) => [...tr.children].map((td) => td.textContent)));
  if (tally.length !== c.tallies.length) problems.push(`${what}: ${tally.length} tally rows, CLI has ${c.tallies.length}`);
  c.tallies.forEach((t, i) => {
    const want = [c.others[i].label, String(t.better), String(t.worse), String(t.not_clear)];
    const got = (tally[i] || []).slice(0, 4);
    if (JSON.stringify(want) !== JSON.stringify(got)) problems.push(`${what}: tally row ${i}: want ${want} got ${got}`);
  });
  const cells = await page.$$eval('#heat tbody tr', (trs) => trs.filter((tr) => tr.querySelector('th button')).map((tr) => ({
    name: tr.querySelector('th button').textContent,
    cells: [...tr.querySelectorAll('td .cell')].map((s) => ({ cls: s.className.replace('cell ', ''), text: s.firstChild ? s.firstChild.textContent : '' })),
  })));
  const byName = new Map(cells.map((r) => [r.name, r.cells]));
  const metrics = new Set(c.rows.map((r) => r.name));
  if (byName.size !== metrics.size) problems.push(`${what}: heatmap has ${byName.size} metrics, CLI ${metrics.size}`);
  // the cell text is the diff formatted by format.js pct(); check it against the CLI's diff too
  const pct = (d) => (d == null ? '—' : (d > 0 ? '+' : d < 0 ? '−' : '') + (Math.abs(d) >= 10 ? Math.abs(d).toFixed(0) : Math.abs(d).toFixed(1)) + '%');
  for (const r of c.rows) {
    const got = (byName.get(r.name) || [])[r.other];
    const want = r.cross_arch ? 'cross' : r.verdict;
    if (!got || got.cls !== want) problems.push(`${what}: heatmap ${r.name} side ${r.other}: want ${want} got ${got && got.cls}`);
    else if (got.text.replace('·', '') !== pct(r.diff)) problems.push(`${what}: heatmap ${r.name} side ${r.other}: text ${got.text}, diff ${r.diff}`);
  }
  const warnings = await page.$$eval('#warnings li', (lis) => lis.map((l) => l.textContent));
  if (JSON.stringify(warnings) !== JSON.stringify(c.warnings)) problems.push(`${what}: warnings ${JSON.stringify(warnings)} vs CLI ${JSON.stringify(c.warnings)}`);
  console.log(`${what}: ${c.tallies.length} tally rows and ${c.rows.length} heatmap cells checked against ${path.basename(cliPath)}`);
  return c;
}

// 1. first load: the exported selection
await page.goto(fileUrl);
await settle();
const meta = await page.$eval('.export-meta', (el) => el.textContent);
if (!/Static export of benchlens \d+\.\d+\.\d+/.test(meta)) problems.push(`export metadata missing: ${meta}`);
if (await page.isVisible('#export-html')) problems.push('the Export button shows in an export');
if (await page.isVisible('nav a[data-view="runs"]')) problems.push('the Runs view is offered in an export');
const exported = await checkAgainst('exported selection', exportedJson);

// 2. the metric view: one dot per value, medians from the comparison
{
  const row = exported.rows.find((r) => r.id === 'pg.pgbench.tpcb.tps') || exported.rows[0];
  await page.click(`#heat tbody th button >> text="${row.name}"`);
  await settle();
  const dots = await page.$$eval('#dots circle', (cs) => cs.length);
  const want = await page.evaluate(([id, refs]) => {
    const data = JSON.parse(document.getElementById('benchlens-data').textContent);
    return refs.reduce((n, ref) => n + data.sides.find((s) => s.side.reference === ref).values.filter((v) => v.id === id).length, 0);
  }, [row.id, [exported.base.reference, ...exported.others.map((o) => o.reference)]]);
  if (!dots || dots !== want) problems.push(`metric view ${row.id}: ${dots} dots, ${want} values`);
  const medians = await page.$$eval('#metric-stats tbody tr', (trs) => trs.map((tr) => tr.children[1].textContent));
  if (medians.length !== 1 + exported.others.length) problems.push(`metric view: ${medians.length} stats rows`);
  console.log(`metric view: ${dots} dots for ${row.id}`);
  await page.click('#back-compare');
  await settle();
}

// 3. hardware of the runs of the comparison (from the inlined run details)
{
  await page.click('nav a[data-view="hardware"]');
  await settle();
  await page.click('#hw-from-compare');
  await settle();
  const cols = await page.$$eval('#hw-table thead th', (ths) => ths.length);
  const runs = [exported.base, ...exported.others].flatMap((s) => s.run_ids);
  if (cols !== new Set(runs).size + 1) problems.push(`hardware view: ${cols - 1} runs shown, comparison has ${new Set(runs).size}`);
  console.log(`hardware view: ${cols - 1} runs`);
  await page.click('nav a[data-view="compare"]');
  await settle();
}

// 4. change the baseline like a reader: first compared side becomes the baseline, the old one is compared
if (swappedJson) {
  const oldBase = exported.base.reference, newBase = exported.others[0].reference;
  await page.selectOption('#base', newBase);
  await settle();
  for (const cb of await page.$$('#pick-groups input:checked, #pick-runs input:checked')) await cb.uncheck();
  await settle();
  await page.check(`input[value="${oldBase}"]`);
  await settle();
  await checkAgainst('baseline changed in the page', swappedJson);
}

// 5. other selections by link
for (let i = 0; i + 1 < pairs.length; i += 2) {
  await page.goto(fileUrl + '?' + pairs[i]);
  await settle();
  await checkAgainst(pairs[i], pairs[i + 1]);
}

// 6. hostile labels: shown as text, never run
if (hostile) {
  const ran = await page.evaluate(() => window.__benchlens_pwned === true || document.querySelector('img[onerror], #heat img, #tally img') !== null);
  if (ran) problems.push('hostile label was interpreted as HTML');
  const labels = await page.$$eval('#tally tbody th', (ths) => ths.map((t) => t.textContent));
  if (!labels.some((l) => l.includes('</script>'))) problems.push(`hostile label not shown as text: ${labels}`);
  console.log('hostile labels: shown as text, nothing ran');
}

if (requests.length) problems.push(`requests other than the file: ${requests.join(', ')}`);
await browser.close();
if (problems.length) {
  console.error('FAIL\n' + problems.map((p) => '  ' + p).join('\n'));
  process.exit(1);
}
console.log('ok: the export works offline and matches benchlens compare');
