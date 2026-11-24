// Open a static HTML export (`benchlens export html`) from disk in headless Chromium with the network off, and
// check that what it shows equals `benchlens compare --json` for the same sides. Run by tests/export/run.sh,
// which builds the inputs; needs the `playwright` npm package and its chromium (not a project dependency).
//
//   node tests/export/offline.mjs REPORT.html EXPORTED.json [SWAPPED.json] [QUERY CLI.json]... [--hostile]
//                                 [--prices=PRICES.json] [--no-price=PLAIN.html]
//
// EXPORTED.json: `benchlens compare BASE OTHER... --json` for the exported selection (checked on first load).
// SWAPPED.json: the comparison with the first compared side as the baseline and the old baseline as the only
// compared side; reached by using the baseline picker and the checkboxes like a reader would.
// QUERY CLI.json: open REPORT.html?QUERY (e.g. `base=RUN_ID&other=group:NAME`) and check against CLI.json.
// The report opens on the Summary view (the first view of an export): its blocks are checked against the CLI
// comparison and score.js (the same module the live UI uses), also for a profile and custom weights in the link.
// --prices=PRICES.json: `{side label: price}`, the example prices set on the runs of REPORT.html; the price block
// must show them. --no-price=PLAIN.html: an export of the same sides before any price was set; its price block
// is hidden, with the reason in the notes.
// --hostile: the export holds labels such as `</script><img onerror=...>`; check nothing ran and the text shows
// as text.

import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const { chromium } = createRequire(import.meta.url)('playwright');
const { weightsFor, scoresFor } = await import(pathToFileURL(path.resolve(import.meta.dirname, '../../web/js/score.js')).href);

const args = process.argv.slice(2);
const hostile = args.includes('--hostile');
const plainArg = args.find((a) => a.startsWith('--no-price='));
const plain = plainArg ? plainArg.slice('--no-price='.length) : null;
const pricesArg = args.find((a) => a.startsWith('--prices='));
const pricesFile = pricesArg ? pricesArg.slice('--prices='.length) : null;
const [report, exportedJson, swappedJson, ...pairs] = args.filter((a) => a !== '--hostile' && a !== plainArg && a !== pricesArg);
if (!report || !exportedJson) {
  console.error('usage: node tests/export/offline.mjs REPORT.html EXPORTED.json [SWAPPED.json] [QUERY CLI.json]... [--hostile]');
  process.exit(2);
}
const fileUrl = pathToFileURL(path.resolve(report)).href;
const fileUrls = new Set([fileUrl]);
const problems = [];
const requests = [];

const browser = await chromium.launch();
const ctx = await browser.newContext({ offline: true, viewport: { width: 1280, height: 900 } });
// nothing but the file itself may be loaded
await ctx.route('**/*', (route) => {
  const u = route.request().url();
  if (u.startsWith('file:') && fileUrls.has(u.split('?')[0])) return route.continue();
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


const SLOTS = ['sum-board', 'sum-radar', 'sum-price', 'sum-scaling', 'sum-stability', 'sum-verdicts'];
const SECTION = { cpu: 'CPU', mem: 'Memory', disk: 'Disk', pg: 'PostgreSQL', net: 'Network', web: 'Web' };
const f1 = (x) => x.toFixed(1);

/**
 * The Summary view of the open page against a CLI Comparison: six slots, each drawn or hidden with its reason
 * in #sum-hidden-notes; leaderboard names, scores and counts equal score.js; the verdicts table equals the
 * tallies (what `benchlens compare --json` prints). `price`: 'shown' | 'hidden' | null (not checked).
 */
async function checkSummary(what, cliPath, { profile = 'balanced', w = '', price = null } = {}) {
  const c = JSON.parse(fs.readFileSync(cliPath, 'utf8'));
  await page.waitForSelector('#view-summary:not([hidden]) #sum-board .sum-board-row', { timeout: 15000 });
  await settle();
  const slots = await page.$$eval('#sum-body > *', (els) => els.map((e) => e.id));
  if (JSON.stringify(slots) !== JSON.stringify([...SLOTS, 'sum-hidden-notes'])) problems.push(`${what}: summary slots ${slots}`);
  const hidden = await page.evaluate((ids) => ids.filter((id) => document.getElementById(id).hidden), SLOTS);
  const notes = await page.$$eval('#sum-hidden-notes .sum-note', (els) => els.length);
  if (notes !== hidden.length) problems.push(`${what}: ${hidden.length} hidden blocks, ${notes} notes`);
  for (const id of hidden) if (await page.$eval('#' + id, (e) => e.childElementCount)) problems.push(`${what}: hidden ${id} is not empty`);
  for (const id of ['sum-board', 'sum-radar', 'sum-stability', 'sum-verdicts']) {
    if (hidden.includes(id)) problems.push(`${what}: ${id} is hidden`);
    else if (!(await page.$eval(`#${id} h2`, (e) => e.textContent))) problems.push(`${what}: ${id} has no heading`);
  }
  if (price) {
    const isHidden = hidden.includes('sum-price');
    if (isHidden !== (price === 'hidden')) problems.push(`${what}: price block ${isHidden ? 'hidden' : 'shown'}, want ${price}`);
    if (price === 'hidden') {
      const text = await page.$eval('#sum-hidden-notes', (e) => e.textContent);
      if (!text.includes('Price / performance: add a "price" tag')) problems.push(`${what}: no price note: ${text}`);
    }
  }

  const scores = scoresFor(c, weightsFor(profile, w).weights);
  const want = [{ name: scores.base.label + ' (baseline)', score: 100 }];
  scores.sides.forEach((s, i) => want.push({ name: s.label, score: s.overall.score, detail: `better ${c.tallies[i].better} · worse ${c.tallies[i].worse} · within noise ${c.tallies[i].not_clear}` }));
  const key = (e) => (e.score == null ? -Infinity : e.score);
  want.sort((a, b) => key(b) - key(a));
  const got = await page.$$eval('#sum-board .sum-board-row', (rows) => rows.map((r) => ({
    name: r.querySelector('.sum-board-name').textContent,
    value: r.querySelector('.sum-board-value').textContent,
    detail: r.querySelector('.sum-board-detail').textContent,
  })));
  if (JSON.stringify(got.map((g) => g.name)) !== JSON.stringify(want.map((e) => e.name))) problems.push(`${what}: board order ${got.map((g) => g.name)} want ${want.map((e) => e.name)}`);
  got.forEach((g, i) => {
    const e = want.find((x) => x.name === g.name);
    if (!e) return;
    if (e.score == null ? g.value !== 'no score' : Math.abs(Number(g.value) - e.score) > 0.051 + (Math.abs(e.score - 100) < 10 ? 0 : 0.5)) problems.push(`${what}: board ${g.name}: ${g.value}, score.js ${e.score}`);
    if (e.detail && g.detail !== e.detail) problems.push(`${what}: board ${g.name}: ${g.detail}, want ${e.detail}`);
  });

  await page.$$eval('#sum-verdicts details.sum-table', (ds) => ds.forEach((d) => { d.open = true; }));
  const rows = await page.$$eval('#sum-verdicts table.data tbody tr', (trs) => trs.map((tr) => [...tr.children].map((x) => x.textContent)));
  c.tallies.forEach((t, i) => {
    const mine = rows.filter((r) => r[0] === c.others[i].label);
    for (const [sec, [b, wr, n]] of Object.entries(t.by_section)) {
      const row = mine.find((r) => r[1] === (SECTION[sec] || sec));
      if (!row || JSON.stringify(row.slice(2).map(Number)) !== JSON.stringify([b, wr, n, b + wr + n])) problems.push(`${what}: verdicts ${c.others[i].label} / ${sec}: ${row && row.slice(2)}, CLI ${[b, wr, n]}`);
    }
    const all = mine.find((r) => r[1] === 'All sections');
    if (!all || JSON.stringify(all.slice(2, 5).map(Number)) !== JSON.stringify([t.better, t.worse, t.not_clear])) problems.push(`${what}: verdicts ${c.others[i].label} totals ${all && all.slice(2, 5)}, CLI ${[t.better, t.worse, t.not_clear]}`);
  });
  console.log(`${what}: summary blocks checked (${SLOTS.length - hidden.length} shown, ${hidden.length} hidden: ${hidden.join(', ') || 'none'}); ${got.length} leaderboard rows against score.js, ${c.tallies.length} tallies against the CLI`);
  return { c, scores, hidden };
}

/** The price block's table rows against the example prices set by tests/export/run.sh (side label -> price). */
async function checkPriceBlock(what, c, scores, prices) {
  await page.$$eval('#sum-price details.sum-table', (ds) => ds.forEach((d) => { d.open = true; }));
  const rows = await page.$$eval('#sum-price table.data tbody tr', (trs) => trs.map((tr) => [...tr.children].map((x) => x.textContent)));
  const sides = [{ label: scores.base.label + ' (baseline)', score: 100 }, ...scores.sides.map((s) => ({ label: s.label, score: s.overall.score }))];
  if (rows.length !== sides.length) problems.push(`${what}: price table has ${rows.length} rows, ${sides.length} sides`);
  for (const sd of sides) {
    const row = rows.find((r) => r[0] === sd.label);
    const p = prices[sd.label.replace(' (baseline)', '')];
    if (!row) { problems.push(`${what}: price table has no row for ${sd.label}`); continue; }
    if (row[1] !== String(p)) problems.push(`${what}: ${sd.label} price ${row[1]}, example price ${p}`);
    if (Math.abs(Number(row[2]) - sd.score) > 0.051 + (Math.abs(sd.score - 100) < 10 ? 0 : 0.5)) problems.push(`${what}: ${sd.label} score ${row[2]}, score.js ${f1(sd.score)}`);
  }
  console.log(`${what}: price block with ${rows.length} priced sides checked`);
}

// 0. first load: an export opens on the Summary view, with the six blocks (or their hidden notes)
await page.goto(fileUrl);
await settle();
if (await page.getAttribute('nav a[data-view]', 'data-view') !== 'summary') problems.push('Summary is not the first view of the export');
if ((await page.getAttribute('nav a[aria-current="page"]', 'data-view')) !== 'summary') problems.push('the export does not open on the Summary view');
{
  const first = await checkSummary('summary, exported selection', exportedJson, { price: pricesFile ? 'shown' : null });
  if (pricesFile) {
    const prices = JSON.parse(fs.readFileSync(pricesFile, 'utf8'));
    await checkPriceBlock('summary, exported selection', first.c, first.scores, prices);
  }
  // a profile and custom weights in the link work offline
  const sides = [first.c.base.reference, ...first.c.others.map((o) => o.reference)];
  const q = `base=${encodeURIComponent(sides[0])}` + sides.slice(1).map((o) => `&other=${encodeURIComponent(o)}`).join('');
  await page.goto(`${fileUrl}?${q}&profile=db`);
  await checkSummary('summary, profile=db link', exportedJson, { profile: 'db' });
  const w = 'cpu:3,disk:1,mem:0,net:1,pg:2,web:0';
  await page.goto(`${fileUrl}?${q}&profile=custom&w=${encodeURIComponent(w)}`);
  await checkSummary('summary, custom weights link', exportedJson, { profile: 'custom', w });
  await page.goto(fileUrl);
  await settle();
}
if (plain) {
  // the same sides exported before any price was set: no price block, one note
  const plainUrl = pathToFileURL(path.resolve(plain)).href;
  fileUrls.add(plainUrl);
  await page.goto(plainUrl);
  await settle();
  await checkSummary('summary without prices', exportedJson, { price: 'hidden' });
  await page.goto(fileUrl);
  await settle();
}

// 1. the compare view of the exported selection (Compare links carry view=compare)
await page.click('nav a[data-view="compare"]');
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
  await page.goto(fileUrl + '?view=compare&' + pairs[i]);
  await settle();
  await checkAgainst(pairs[i], pairs[i + 1]);
  await page.goto(fileUrl + '?' + pairs[i]);
  await checkSummary(pairs[i] + ' (summary)', pairs[i + 1]);
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
