// Headless smoke test of the web UI (development only, needs the `playwright` npm package and its chromium).
//
//   node web/dev/smoke.mjs http://127.0.0.1:8090 OUT_DIR [compare.json]
//
// Opens every view at 1280 px and 400 px, light and dark, saves screenshots to OUT_DIR, fails on console errors,
// page errors or horizontal page scroll. With compare.json (`benchlens compare BASE OTHER... --json` for the
// URL's sides, below) it also checks that the tally table and the heatmap show exactly those numbers.

import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

// require() so NODE_PATH works (e.g. NODE_PATH=~/.npm/_npx/<hash>/node_modules after `npx playwright install`)
const { chromium } = createRequire(import.meta.url)('playwright');

const [base = 'http://127.0.0.1:8090', out = 'smoke-out', cliJson] = process.argv.slice(2);
const SIDES = process.env.SIDES || 'base=group:x86-high&other=group:arm64-high&other=group:x86-med';
fs.mkdirSync(out, { recursive: true });

const pages = [
  ['compare', `?${SIDES}`],
  ['compare-all', `?${SIDES}&section=all`],
  ['metric', `?view=metric&${SIDES}&metric=pg.pgbench.tpcb.tps`],
  ['metric-sysbench', `?view=metric&${SIDES}&metric=cpu.sysbench.prime.max`],
  ['runs', '?view=runs'],
  ['hardware', `?view=hardware&${SIDES}`],
];

const problems = [];
const browser = await chromium.launch();
for (const scheme of ['light', 'dark']) {
  for (const width of [1280, 400]) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
    const page = await ctx.newPage();
    page.on('console', (m) => { if (m.type() === 'error') problems.push(`${scheme} ${width}: console: ${m.text()}`); });
    page.on('pageerror', (e) => problems.push(`${scheme} ${width}: page error: ${e.message}`));
    for (const [name, q] of pages) {
      await page.goto(base + '/' + q);
      await page.waitForLoadState('networkidle');
      if (name === 'hardware') {
        await page.click('#hw-from-compare');
        await page.waitForLoadState('networkidle');
      }
      await page.waitForTimeout(150);
      const sw = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      if (sw > 0) problems.push(`${scheme} ${width} ${name}: page scrolls sideways by ${sw}px`);
      const err = await page.$$eval('.state.error', (els) => els.map((e) => e.textContent));
      if (err.length) problems.push(`${scheme} ${width} ${name}: error shown: ${err.join(' | ')}`);
      await page.screenshot({ path: path.join(out, `${name}-${width}-${scheme}.png`), fullPage: true });
    }
    await ctx.close();
  }
}

if (cliJson) {
  const c = JSON.parse(fs.readFileSync(cliJson, 'utf8'));
  const page = await browser.newPage();
  await page.goto(`${base}/?${SIDES}&section=all`);
  await page.waitForLoadState('networkidle');
  const tally = await page.$$eval('#tally tbody tr', (trs) => trs.map((tr) => [...tr.children].map((td) => td.textContent)));
  c.tallies.forEach((t, i) => {
    const want = [c.others[i].label, String(t.better), String(t.worse), String(t.not_clear)];
    const got = tally[i].slice(0, 4);
    if (JSON.stringify(want) !== JSON.stringify(got)) problems.push(`tally row ${i}: want ${want} got ${got}`);
  });
  // heatmap: every cell's verdict class and every metric row present
  const cells = await page.$$eval('#heat tbody tr', (trs) => trs.filter((tr) => tr.querySelector('th button')).map((tr) => ({
    name: tr.querySelector('th button').textContent,
    cells: [...tr.querySelectorAll('td .cell')].map((s) => s.className.replace('cell ', '')),
  })));
  const byName = new Map(cells.map((r) => [r.name, r.cells]));
  let checked = 0;
  for (const r of c.rows) {
    const got = byName.get(r.name);
    const want = r.cross_arch ? 'cross' : r.verdict;
    if (!got || got[r.other] !== want) problems.push(`heatmap ${r.name} side ${r.other}: want ${want} got ${got && got[r.other]}`);
    checked++;
  }
  console.log(`checked ${c.tallies.length} tally rows and ${checked} heatmap cells against ${cliJson}`);
}
await browser.close();
if (problems.length) { console.error(problems.join('\n')); process.exit(1); }
console.log('smoke ok; screenshots in', out);
