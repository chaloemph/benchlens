// Token auth and upload against two more servers started by scripts/e2e.sh on copies of the database without
// x86-std-r2: BENCHLENS_TOKEN_URL (token, public reads) and BENCHLENS_PRIVATE_URL (token, read_public=false).
// The open server of the other files listens on loopback without a token, where writes stay open.

import fs from 'node:fs';
import { test, expect } from '@playwright/test';

const TOKEN = process.env.BENCHLENS_E2E_TOKEN;
const TOKEN_URL = (process.env.BENCHLENS_TOKEN_URL || '').replace(/\/?$/, '/');
const PRIVATE_URL = (process.env.BENCHLENS_PRIVATE_URL || '').replace(/\/?$/, '/');
const UPLOAD = process.env.BENCHLENS_UPLOAD;
const NO_REPORT = process.env.BENCHLENS_NO_REPORT || 'x86-std-r2-20261119-103253';
const bearer = { Authorization: 'Bearer ' + TOKEN };

test.skip(!TOKEN || !process.env.BENCHLENS_TOKEN_URL || !process.env.BENCHLENS_PRIVATE_URL, 'run through scripts/e2e.sh');

/** Page errors and console errors, except the 401s the token flow expects (shown in the console by Chromium). */
function watch(page) {
  const errors = [];
  page.on('pageerror', (e) => errors.push('page error: ' + e.message));
  page.on('console', (m) => { if (m.type() === 'error' && !/\b401\b/.test(m.text())) errors.push('console: ' + m.text()); });
  return errors;
}

async function answerToken(page, value, expectError) {
  const dlg = page.locator('#token-dialog');
  await expect(dlg).toBeVisible();
  if (expectError) await expect(dlg.locator('#token-error')).toContainText(expectError);
  await dlg.locator('#token-input').fill(value);
  await dlg.getByRole('button', { name: 'Use token' }).click();
}

test('token server: writes without the token are 401, reads stay open', async ({ request }) => {
  const runs = await (await request.get(TOKEN_URL + 'api/runs')).json();
  expect(runs.length).toBeGreaterThan(0);
  const id = runs[0].run_id;
  for (const [method, path, data] of [['PATCH', `api/runs/${id}/tags`, { e2e: 'x' }], ['DELETE', `api/runs/${id}`]]) {
    for (const headers of [{}, { Authorization: 'Bearer wrong-token-0123456789' }]) {
      const res = await request.fetch(TOKEN_URL + path, { method, data, headers });
      expect(res.status(), `${method} ${path}`).toBe(401);
      expect(res.headers()['www-authenticate']).toContain('Bearer');
    }
  }
  const res = await request.post(TOKEN_URL + 'api/runs', { multipart: { file: { name: 'x.tar.gz', mimeType: 'application/gzip', buffer: Buffer.from('x') } } });
  expect(res.status()).toBe(401);
  expect((await (await request.get(TOKEN_URL + 'api/runs')).json()).length).toBe(runs.length);
});

test('token server: upload through the dialog asks for the token, refuses a wrong one, then imports', async ({ page, request }) => {
  const errors = watch(page);
  const before = await (await request.get(TOKEN_URL + 'api/runs')).json();
  expect(before.some((r) => r.folder === NO_REPORT), `${NO_REPORT} is not imported yet`).toBe(false);
  await page.goto(TOKEN_URL + '?view=runs');
  await expect(page.locator('#runs-table tbody tr').first()).toBeVisible();

  await page.getByRole('button', { name: 'Upload' }).click();
  const dlg = page.locator('#upload-dialog');
  await expect(dlg).toBeVisible();
  await dlg.locator('#upload-file').setInputFiles(UPLOAD);
  await dlg.getByRole('button', { name: 'Add tag' }).click();
  await dlg.locator('#upload-tags .row').last().locator('.tag-key').fill('e2e');
  await dlg.locator('#upload-tags .row').last().locator('.tag-value').fill('uploaded');
  await dlg.locator('#upload-submit').click();

  await answerToken(page, 'wrong-token-0123456789');
  await answerToken(page, TOKEN, 'That token was refused');
  await expect(dlg.locator('.upload-result.imported')).toHaveCount(1);
  await expect(dlg.locator('.upload-result.imported')).toContainText(NO_REPORT);
  await dlg.getByRole('button', { name: 'Close' }).click();

  const after = await (await request.get(TOKEN_URL + 'api/runs')).json();
  const run = after.find((r) => r.folder === NO_REPORT);
  expect(run, 'uploaded run listed').toBeTruthy();
  expect(run.tags.e2e).toBe('uploaded');
  await expect(page.locator(`#runs-table tr[data-run="${run.run_id}"]`)).toHaveCount(1);

  // the same archive through the API with the token: already present, nothing added
  const again = await request.post(TOKEN_URL + 'api/runs', {
    headers: bearer,
    multipart: { file: { name: 'again.tar.gz', mimeType: 'application/gzip', buffer: fs.readFileSync(UPLOAD) } },
  });
  expect(again.status()).toBe(200);
  expect((await again.json()).map((r) => r.outcome)).toEqual(['already_present']);
  expect((await (await request.get(TOKEN_URL + 'api/runs')).json()).length).toBe(after.length);
  expect(errors).toEqual([]);
});

test('private server: every /api read but health needs the token; the page asks for it', async ({ page, request }) => {
  expect((await request.get(PRIVATE_URL + 'api/health')).status()).toBe(200);
  for (const path of ['api/runs', 'api/groups', 'api/export.html?base=group:x86-high&other=group:arm64-high']) {
    const res = await request.get(PRIVATE_URL + path);
    expect(res.status(), path).toBe(401);
  }
  expect((await request.get(PRIVATE_URL + 'api/runs', { headers: bearer })).status()).toBe(200);

  const errors = watch(page);
  await page.goto(PRIVATE_URL + '?view=compare');
  await answerToken(page, TOKEN);
  await expect(page.locator('#heat tbody tr').first()).toBeVisible();
  await expect(page.locator('.state.error')).toHaveCount(0);

  // the Export button sends the token too
  const download = page.waitForEvent('download');
  await page.locator('#export-html').click();
  const file = await (await download).path();
  const html = fs.readFileSync(file, 'utf8');
  expect(html).toContain('id="benchlens-data"');
  expect(html).toContain('Content-Security-Policy');
  expect(errors).toEqual([]);
});

test('private server: report.md opens from the Runs view with the token', async ({ page, request }) => {
  const runs = await (await request.get(PRIVATE_URL + 'api/runs', { headers: bearer })).json();
  const r = runs.find((x) => x.kept_files.includes('report.md'));
  expect(r, 'a run with report.md').toBeTruthy();
  const file = PRIVATE_URL + 'api/runs/' + r.run_id + '/files/report.md';
  expect((await request.get(file)).status()).toBe(401);
  const text = await (await request.get(file, { headers: bearer })).text();

  const errors = watch(page);
  await page.goto(PRIVATE_URL + '?view=runs');
  await answerToken(page, TOKEN);
  const link = page.locator(`#runs-table tr[data-run="${r.run_id}"] .report-slot a`);
  await link.click();
  const dlg = page.locator('#report-dialog');
  await expect(dlg).toBeVisible();
  await expect(dlg.locator('pre.report-text')).toHaveText(text.replace(/\n$/, ''));
  await expect(dlg.locator('.state.error')).toHaveCount(0);
  await dlg.getByRole('button', { name: 'Close' }).click();
  await expect(dlg).toBeHidden();
  expect(errors).toEqual([]);
});
