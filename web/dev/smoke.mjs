// Headless smoke test of the web UI (development only, needs the `playwright` npm package and its chromium).
//
//   node web/dev/smoke.mjs http://127.0.0.1:8080 OUT_DIR [compare.json]   (against `benchlens serve`)
//
// Opens every view at 1280 px and 400 px, light and dark, saves screenshots to OUT_DIR, fails on console errors,
// page errors or horizontal page scroll. With compare.json (`benchlens compare BASE OTHER... --json` for the
// URL's sides, below) it also checks that the tally table and the heatmap show exactly those numbers.
//
// With BENCHLENS_TOKEN (the token the server was started with) it also checks auth and upload: writes without
// the token are 401, and UPLOAD (a .tar.gz of result folders) goes through the Upload dialog, the token prompt
// (a wrong token first, then the right one) and lists its import reports. With READ_PRIVATE=1 (server started
// with read_public=false) the first page load must ask for the token before showing anything.
// It also checks the runs view: tag drafts survive re-renders, report.md links, delete dialog text (NO_REPORT=folder).

import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

// require() so NODE_PATH works (e.g. NODE_PATH=~/.npm/_npx/<hash>/node_modules after `npx playwright install`)
const { chromium } = createRequire(import.meta.url)('playwright');

const [base = 'http://127.0.0.1:8080', out = 'smoke-out', cliJson] = process.argv.slice(2);
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
// a private server (READ_PRIVATE=1) asks for the token on every new page: only the auth checks below run
for (const scheme of process.env.READ_PRIVATE ? [] : ['light', 'dark']) {
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

// Runs view interactions (light, 1280): tag drafts survive re-renders, report.md is linked only for runs that
// kept it, the delete dialog says what deleting does. NO_REPORT names a folder that has no report.md (must exist).
{
  const NO_REPORT = process.env.NO_REPORT || 'x86-std-r2-20261119-103253';
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on('pageerror', (e) => problems.push(`runs: page error: ${e.message}`));
  await page.goto(base + '/?view=runs');
  await page.waitForSelector('#runs-table tbody tr[data-run]');
  const ids = await page.$$eval('#runs-table tbody tr[data-run]', (trs) => trs.map((tr) => tr.dataset.run));
  const row = (id) => `#runs-table tr[data-run="${id}"]`;

  // 1. draft: open the form, add a tag, type, then re-render in three ways
  const [a, b] = ids;
  await page.click(`${row(a)} button.edit-tags`);
  await page.click(`${row(a)} .tag-form button:text("+ tag")`);
  await page.keyboard.type('smoke_key');
  await page.keyboard.press('Tab');
  await page.keyboard.type('draft-val');
  const draftOk = async (what, wantFocus) => {
    const st = await page.evaluate((sel) => {
      const ins = [...document.querySelectorAll(sel + ' .tag-form input')];
      const last = ins.slice(-2).map((i) => i.value);
      return { last, focused: document.activeElement === ins[ins.length - 1], caret: ins[ins.length - 1]?.selectionStart };
    }, row(a));
    if (JSON.stringify(st.last) !== JSON.stringify(['smoke_key', 'draft-val'])) problems.push(`runs: draft lost after ${what}: ${JSON.stringify(st.last)}`);
    if (wantFocus && (!st.focused || st.caret !== 'draft-val'.length)) problems.push(`runs: focus/caret lost after ${what}`);
  };
  await page.evaluate(() => window.dispatchEvent(new Event('statechange')));   // what a reload or filter change does
  await draftOk('a re-render while typing', true);
  await page.keyboard.type('!');
  await page.keyboard.press('Backspace');
  await page.check(`${row(b)} input[type=checkbox]`);
  await draftOk('ticking a checkbox', false);
  if (!(await page.isChecked(`${row(b)} input[type=checkbox]`))) problems.push('runs: checkbox not kept');
  await page.selectOption('#runs-arch', { index: 0 });
  await page.fill('#runs-q', '');
  await page.dispatchEvent('#runs-q', 'input');
  await draftOk('a filter change', false);
  await page.click(`${row(a)} .tag-form button:text("Cancel")`);
  if (await page.$(`${row(a)} .tag-form`)) problems.push('runs: Cancel did not close the form');
  if (!(await page.evaluate((sel) => document.activeElement === document.querySelector(sel + ' button.edit-tags'), row(a)))) problems.push('runs: focus not back on Edit tags after Cancel');

  // 2. report.md: scroll every row into view, wait for the lookups, compare with each run's file list
  for (const id of ids) await page.$eval(row(id), (tr) => tr.scrollIntoView({ block: 'center' }));
  await page.waitForFunction(() => !document.querySelector('#runs-table .pending-report'), null, { timeout: 10000 })
    .catch(() => problems.push('runs: report.md lookups did not finish'));
  const links = await page.evaluate(async (list) => Promise.all(list.map(async (id) => {
    const d = await (await fetch('api/runs/' + encodeURIComponent(id))).json();
    const tr = document.querySelector(`#runs-table tr[data-run="${CSS.escape(id)}"]`);
    return { id, folder: d.folder, kept: d.files.some((f) => f.name === 'report.md'), linked: !!tr.querySelector('.report-slot a[href]') };
  })), ids);
  for (const l of links) if (l.kept !== l.linked) problems.push(`runs: ${l.folder}: report.md kept=${l.kept} but linked=${l.linked}`);
  const nr = links.find((l) => l.folder === NO_REPORT);
  if (!nr) problems.push(`runs: no run with folder ${NO_REPORT} (NO_REPORT) to check the missing report.md`);
  else if (nr.kept || nr.linked) problems.push(`runs: ${NO_REPORT} should have no report.md link`);
  if (!links.some((l) => l.linked)) problems.push('runs: no run links report.md at all (import one that kept it)');

  // 3. delete dialog text, then cancel
  await page.click(`${row(a)} button:text("Delete")`);
  const text = await page.$eval('#confirm-dialog', (d) => d.open ? d.textContent.replace(/\s+/g, ' ') : '');
  for (const want of ['removes the run and its values', 'Importing the same folder again brings the values back',
    'tags edited here or given by a registry are not restored unless they are given again']) {
    if (!text.includes(want)) problems.push(`runs: delete dialog lacks "${want}": ${text}`);
  }
  await page.click('#confirm-dialog button[value="cancel"]');
  await page.waitForTimeout(100);
  if (!(await page.$(row(a)))) problems.push('runs: Cancel in the delete dialog deleted the run');
  await page.close();
  console.log(`checked runs view: drafts, report.md links of ${links.length} runs, delete dialog`);
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
const TOKEN = process.env.BENCHLENS_TOKEN;
if (TOKEN) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => problems.push(`auth: page error: ${e.message}`));
  page.on('console', (m) => {
    // the browser logs refused requests (401) as console errors; those are expected here
    if (m.type() === 'error' && !/401|Unauthorized/.test(m.text())) problems.push(`auth: console: ${m.text()}`);
  });
  await page.goto(base + '/?view=runs');
  if (process.env.READ_PRIVATE) {
    // reads need the token: the prompt opens before any data shows
    await page.waitForSelector('#token-dialog[open]', { timeout: 5000 });
    await page.fill('#token-input', TOKEN);
    await page.click('#token-dialog button[value="ok"]');
  }
  await page.waitForLoadState('networkidle');
  await page.waitForSelector('#runs-table tbody tr', { timeout: 10000 });
  const st = await page.evaluate(async () => {
    const r = await fetch('api/runs/nope/tags', { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: '{}' });
    return [r.status, r.headers.get('www-authenticate')];
  });
  if (st[0] !== 401 || !/^Bearer/.test(st[1] || '')) problems.push(`auth: PATCH without token gave ${st}`);
  const upload = process.env.UPLOAD;
  if (upload) {
    if (process.env.READ_PRIVATE) await page.evaluate(() => sessionStorage.removeItem('benchlens-token'));
    await page.click('#upload-open');
    await page.waitForSelector('#upload-dialog[open]');
    await page.setInputFiles('#upload-file', upload);
    await page.click('#upload-add-tag');
    await page.fill('#upload-tags .row:last-child .tag-key', 'smoke');
    await page.fill('#upload-tags .row:last-child .tag-value', 'ui-upload');
    await page.click('#upload-submit');
    // no token yet: the prompt opens; a wrong token is refused and the prompt says so
    await page.waitForSelector('#token-dialog[open]');
    await page.fill('#token-input', 'wrong-token-for-the-smoke-test');
    await page.click('#token-dialog button[value="ok"]');
    await page.waitForSelector('#token-dialog[open] #token-error:not([hidden])');
    await page.fill('#token-input', TOKEN);
    await page.click('#token-dialog button[value="ok"]');
    await page.waitForSelector('#upload-results li.upload-result', { timeout: 30000 });
    const results = await page.$$eval('#upload-results li.upload-result', (lis) => lis.map((li) => li.className.replace('upload-result ', '')));
    const err = await page.$eval('#upload-error', (e) => (e.hidden ? '' : e.textContent));
    if (err || !results.length || results.some((r) => r === 'failed')) problems.push(`upload: ${results} ${err}`);
    await page.screenshot({ path: path.join(out, 'upload-1280.png'), fullPage: true });
    await page.click('#upload-dialog button:has-text("Close")');
    await page.waitForLoadState('networkidle');
    const tagged = await page.$$eval('#runs-table .chip', (cs) => cs.some((c) => c.textContent.includes('ui-upload')));
    if (!tagged) problems.push('upload: the runs table does not show the tag set in the upload dialog');
    console.log(`uploaded ${upload} through the UI: ${results.join(', ')}`);
  }
  await ctx.close();
}

await browser.close();
if (problems.length) { console.error(problems.join('\n')); process.exit(1); }
console.log('smoke ok; screenshots in', out);
