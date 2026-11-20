// Every read endpoint of docs/api.md answers with the documented shape; the documented error cases.

import { test, expect } from '@playwright/test';
import {
  api, expectShape, expectStringMap, expectApiError, fixtureFolders, pickComparisons, sidesQuery, SHAPES,
} from './helpers.js';

const VERDICTS = ['better', 'worse', 'not_clear'];
const DIRECTIONS = ['higher', 'lower'];

test.describe('API v1 read endpoints', () => {
  test('GET /api/health counts every imported fixture', async ({ request }) => {
    const h = await api(request, 'api/health');
    expectShape(h, SHAPES.health, 'health');
    expect(h.runs).toBe(fixtureFolders().length);
  });

  test('GET /api/runs: RunSummary[] oldest first, one per fixture', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    expect(runs.map((r) => r.folder).sort()).toEqual(fixtureFolders().sort());
    runs.forEach((r, i) => {
      expectShape(r, SHAPES.runSummary, `runs[${i}]`);
      expectStringMap(r.tags, `runs[${i}].tags`);
      expect(['cli', 'inbox', 'upload', 'api']).toContain(r.source);
    });
    const starts = runs.map((r) => r.start_utc);
    expect(starts).toEqual([...starts].sort());
    // the registry tagged every fixture
    for (const r of runs) expect(r.tags.round, `${r.folder}: round tag from the registry`).toBeTruthy();
  });

  test('GET /api/runs filters: arch, group, tag, q', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    const r0 = runs[0];
    const byArch = await api(request, 'api/runs?arch=' + encodeURIComponent(r0.arch));
    expect(byArch.map((r) => r.run_id)).toEqual(runs.filter((r) => r.arch === r0.arch).map((r) => r.run_id));
    const byGroup = await api(request, 'api/runs?group=' + encodeURIComponent(r0.group));
    expect(byGroup.map((r) => r.run_id)).toEqual(runs.filter((r) => r.group === r0.group).map((r) => r.run_id));
    const byTag = await api(request, 'api/runs?tag=' + encodeURIComponent('round:' + r0.tags.round));
    expect(byTag.map((r) => r.run_id)).toEqual(runs.filter((r) => r.tags.round === r0.tags.round).map((r) => r.run_id));
    const both = await api(request, `api/runs?tag=${encodeURIComponent('round:' + r0.tags.round)}&arch=${encodeURIComponent(r0.arch)}`);
    expect(both.map((r) => r.run_id)).toEqual(runs.filter((r) => r.tags.round === r0.tags.round && r.arch === r0.arch).map((r) => r.run_id));
    const q = r0.folder.toUpperCase();
    const byQ = await api(request, 'api/runs?q=' + encodeURIComponent(q));
    expect(byQ.map((r) => r.run_id)).toContain(r0.run_id);
    expect(await api(request, 'api/runs?q=no-such-run-anywhere')).toEqual([]);
  });

  test('GET /api/runs/{id}: RunDetail by id, prefix, folder and label', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    for (const r of runs) {
      const d = await api(request, 'api/runs/' + r.run_id);
      expectShape(d, SHAPES.runDetail, `run ${r.folder}`);
      expectStringMap(d.tags, `run ${r.folder}.tags`);
      // every RunSummary field is the same as in the list
      for (const k of Object.keys(SHAPES.runSummary)) expect(d[k], `${r.folder}.${k}`).toEqual(r[k]);
      expect(d.files.map((f) => f.name)).toContain('meta.json');
      for (const f of d.files) {
        const res = await request.get(`api/runs/${r.run_id}/files/${f.name}`);
        expect(res.status(), `${r.folder}/${f.name}`).toBe(200);
        expect(res.headers()['content-type']).toContain(f.name.endsWith('.json') ? 'application/json' : 'text/plain');
        expect((await res.body()).length, `${r.folder}/${f.name} bytes`).toBe(f.bytes);
      }
    }
    const r = runs[0];
    for (const ref of [r.run_id.slice(0, 6), r.folder]) {
      expect((await api(request, 'api/runs/' + encodeURIComponent(ref))).run_id, ref).toBe(r.run_id);
    }
    const uniqueLabel = runs.find((x) => runs.filter((y) => y.label === x.label).length === 1);
    if (uniqueLabel) expect((await api(request, 'api/runs/' + encodeURIComponent(uniqueLabel.label))).run_id).toBe(uniqueLabel.run_id);
  });

  test('GET /api/groups: GroupSummary[] covering every run', async ({ request }) => {
    const groups = await api(request, 'api/groups');
    const runs = await api(request, 'api/runs');
    groups.forEach((g, i) => expectShape(g, SHAPES.group, `groups[${i}]`));
    expect(groups.map((g) => g.group).sort()).toEqual([...new Set(runs.map((r) => r.group))].sort());
    for (const g of groups) {
      const mine = runs.filter((r) => r.group === g.group);
      expect(g.runs, g.group).toBe(mine.length);
      expect([...g.labels].sort(), g.group).toEqual(mine.map((r) => r.label).sort());
      expect([...g.archs].sort(), g.group).toEqual([...new Set(mine.map((r) => r.arch))].sort());
    }
  });

  test('GET /api/compare: Comparison for both comparisons under test', async ({ request }) => {
    for (const cmp of await pickComparisons(request)) {
      const c = await api(request, 'api/compare?' + sidesQuery(cmp));
      expectShape(c, { base: SHAPES.side, others: [SHAPES.side], rows: [SHAPES.row], tallies: [SHAPES.tally], warnings: ['string'] }, cmp.name);
      expect(c.base.reference).toBe(cmp.base);
      expect(c.others.map((o) => o.reference)).toEqual(cmp.others);
      expect(c.tallies).toHaveLength(c.others.length);
      expect(c.rows.length, cmp.name).toBeGreaterThan(0);
      for (const r of c.rows) {
        expect(VERDICTS).toContain(r.verdict);
        expect(DIRECTIONS).toContain(r.direction);
        expect(r.other).toBeLessThan(c.others.length);
      }
      // tallies count the rows that are not cross_arch, per section too
      c.tallies.forEach((t, k) => {
        const counted = c.rows.filter((r) => r.other === k && !r.cross_arch);
        for (const v of VERDICTS) expect(t[v], `${cmp.name} ${v}`).toBe(counted.filter((r) => r.verdict === v).length);
        for (const [sec, arr] of Object.entries(t.by_section)) {
          expect(arr, `${cmp.name} by_section.${sec}`).toEqual(VERDICTS.map((v) => counted.filter((r) => r.section === sec && r.verdict === v).length));
        }
      });
      // section= keeps only that section's rows, tallies stay complete
      const sec = c.rows[0].section;
      const only = await api(request, `api/compare?${sidesQuery(cmp)}&section=${sec}`);
      expect(only.rows).toEqual(c.rows.filter((r) => r.section === sec));
      expect(only.tallies).toEqual(c.tallies);
    }
  });

  test('GET /api/values: ValueRow[] per side; unknown metric gives []', async ({ request }) => {
    const [cmp] = await pickComparisons(request);
    const c = await api(request, 'api/compare?' + sidesQuery(cmp));
    const id = c.rows[0].id;
    for (const side of [c.base, ...c.others]) {
      const v = await api(request, `api/values?ref=${encodeURIComponent(side.reference)}&id=${encodeURIComponent(id)}`);
      expectShape(v, { side: SHAPES.side, id: 'string', values: [SHAPES.valueRow] }, 'values ' + side.reference);
      expect(v.id).toBe(id);
      expect(v.side.archs).toEqual([]);
      expect(v.side.run_ids).toEqual(side.run_ids);
      expect(v.values.length).toBeGreaterThan(0);
      for (const row of v.values) {
        expect(row.id).toBe(id);
        expect(side.run_ids).toContain(row.run_id);
      }
    }
    const none = await api(request, `api/values?ref=${encodeURIComponent(cmp.base)}&id=no.such.metric`);
    expect(none.values).toEqual([]);
  });

  test('GET /api/metrics: Metric[] of every imported vmbench version', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    for (const version of [...new Set(runs.map((r) => r.vmbench_version))]) {
      const m = await api(request, 'api/metrics?version=' + encodeURIComponent(version));
      expect(m.length, version).toBeGreaterThan(0);
      m.forEach((x, i) => expectShape(x, SHAPES.metric, `metrics ${version}[${i}]`));
      // `none`: informational, never judged (docs/contract.md), so never in a comparison row
      for (const x of m) expect([...DIRECTIONS, 'none']).toContain(x.direction);
      const bare = await api(request, 'api/metrics?version=' + encodeURIComponent(version.replace(/^v/, '')));
      expect(bare).toEqual(m);
    }
  });
});

test.describe('API v1 errors', () => {
  test('404 for an unknown run, in every place a run reference goes', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    await expectApiError(request, 'api/runs/no-such-run', 404);
    await expectApiError(request, 'api/runs/no-such-run/files/meta.json', 404);
    await expectApiError(request, `api/compare?base=no-such-run&other=${runs[0].run_id}`, 404);
    await expectApiError(request, `api/compare?base=${runs[0].run_id}&other=no-such-run`, 404);
    await expectApiError(request, 'api/values?ref=no-such-run&id=x', 404);
    await expectApiError(request, 'api/compare?base=group:no-such-group&other=' + runs[0].run_id, 404);
    await expectApiError(request, `api/runs/${runs[0].run_id}/files/not-a-kept-file.txt`, 404);
    await expectApiError(request, 'api/metrics?version=0.0.0', 404);
    await expectApiError(request, 'api/no-such-endpoint', 404);
  });

  test('400 for an unknown query key', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    await expectApiError(request, 'api/runs?nope=1', 400);
    await expectApiError(request, `api/compare?base=${runs[0].run_id}&other=${runs[1].run_id}&nope=1`, 400);
    await expectApiError(request, `api/values?ref=${runs[0].run_id}&id=x&nope=1`, 400);
    await expectApiError(request, 'api/metrics?version=0.2.3&nope=1', 400);
    // a single-valued key twice
    await expectApiError(request, `api/compare?base=${runs[0].run_id}&base=${runs[1].run_id}&other=${runs[1].run_id}`, 400);
  });

  test('400 for compare without other, metrics without version, group: as a run id', async ({ request }) => {
    const runs = await api(request, 'api/runs');
    await expectApiError(request, 'api/compare?base=' + runs[0].run_id, 400);
    await expectApiError(request, 'api/metrics', 400);
    await expectApiError(request, 'api/runs/' + encodeURIComponent('group:' + runs[0].group), 400);
  });
});
