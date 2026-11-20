// Runs view: every run with search and filters, tags (inline editing via PATCH), delete, report.md link.
//
// The table is rebuilt on every render (checkbox, filter, reload after a write). Open tag forms keep their
// draft in `drafts` (run id -> rows typed so far) and the focused control is put back after the rebuild, so
// nothing typed is lost. The report.md link needs the run's file list, which only RunDetail has: it is fetched
// (shared cache with the hardware view) when the row first scrolls into view; until then the link is disabled.

import { api } from './api.js';
import { store, sortedGroups } from './data.js';
import { setState, readState } from './state.js';
import { h, clear, stateBlock, errorBlock, confirmDialog } from './ui.js';
import { archName, shortDate } from './format.js';
import { getDetail } from './hardware.js';

const selected = new Set();
/** run id -> { rows: [{k, v}], msg: string, saving: bool } for every open tag form */
const drafts = new Map();
/** run id -> true / false once its file list is known (reset when the runs are reloaded) */
const hasReport = new Map();
let reportFor = null;
let observer = null;
let onChanged = async () => {};

/** `fn` reloads runs and groups after a write. */
export function initRuns(fn) {
  onChanged = fn;
  const q = document.getElementById('runs-q');
  q.addEventListener('input', () => setState({ q: q.value }, { replace: true }));
  for (const [id, key] of [['runs-arch', 'arch'], ['runs-group', 'group'], ['runs-tag', 'tag']]) {
    const sel = document.getElementById(id);
    sel.addEventListener('change', () => setState({ [key]: sel.value }, { replace: true }));
  }
  document.getElementById('runs-hw').addEventListener('click', () => setState({ view: 'hardware', run: [...selected] }));
}

function options(sel, label, values, current, text = (v) => v) {
  const vals = [...values];
  if (current && !vals.includes(current)) vals.push(current);
  clear(sel, h('option', { value: '', text: label }), ...vals.map((v) => h('option', { value: v, text: text(v) })));
  sel.value = current;
}

function matches(r, s) {
  if (s.arch && r.arch !== s.arch) return false;
  if (s.group && r.group !== s.group) return false;
  if (s.tag) {
    const i = s.tag.indexOf(':');
    const k = i < 0 ? s.tag : s.tag.slice(0, i), v = i < 0 ? '' : s.tag.slice(i + 1);
    if ((r.tags[k] ?? null) !== v) return false;
  }
  if (s.q) {
    const n = s.q.toLowerCase();
    const hay = [r.label, r.folder, r.group, r.run_id, ...Object.values(r.tags)];
    if (!hay.some((x) => x.toLowerCase().includes(n))) return false;
  }
  return true;
}

export function renderRuns(s) {
  const q = document.getElementById('runs-q');
  if (document.activeElement !== q) q.value = s.q;
  options(document.getElementById('runs-arch'), 'any', [...new Set(store.runs.map((r) => r.arch))].sort(), s.arch, archName);
  options(document.getElementById('runs-group'), 'any', sortedGroups().map((g) => g.group), s.group);
  const tagVals = [...new Set(store.runs.flatMap((r) => Object.entries(r.tags).filter(([k]) => k !== 'note').map(([k, v]) => k + ':' + v)))].sort();
  options(document.getElementById('runs-tag'), 'any', tagVals, s.tag, (v) => v.replace(':', ' = '));

  for (const id of [...selected]) if (!store.runById.has(id)) selected.delete(id);
  for (const id of [...drafts.keys()]) if (!store.runById.has(id)) drafts.delete(id);
  if (reportFor !== store.runs) { hasReport.clear(); reportFor = store.runs; }
  const runs = store.runs.filter((r) => matches(r, s));
  document.getElementById('runs-count').textContent = `${runs.length} of ${store.runs.length} runs` + (selected.size ? ` · ${selected.size} selected` : '');
  const hw = document.getElementById('runs-hw');
  hw.disabled = !selected.size;
  hw.textContent = selected.size ? `Hardware of selected (${selected.size})` : 'Hardware of selected';

  const table = document.getElementById('runs-table');
  const cols = ['', 'Run', 'Group', 'Arch', 'vCPU', 'RAM GB', 'CPU', 'vmbench', 'Started', 'Values', 'Flags', 'Tags', 'Actions'];
  const head = h('tr', {}, ...cols.map((c, i) => h('th', { scope: 'col', class: i === 0 ? '' : 'left' }, i === 0 ? h('span', { class: 'sr-only', text: 'Select' }) : c)));
  const body = h('tbody');
  for (const r of runs) body.append(runRow(r));
  if (!runs.length) body.append(h('tr', {}, h('td', { colspan: String(cols.length), class: 'left muted', text: 'No run matches these filters.' })));
  const focus = saveFocus(table);
  clear(table, h('thead', {}, head), body);
  restoreFocus(table, focus);
  watchRows(body);
}

const rerender = () => renderRuns(readState());

// Focus survives the rebuild: remember the focused control by run id + its index among the row's controls.
const CONTROLS = 'input, button, a[href], select, textarea';
function saveFocus(table) {
  const el = document.activeElement;
  const tr = el && table.contains(el) ? el.closest('tr[data-run]') : null;
  if (!tr) return null;
  const f = { run: tr.dataset.run, i: [...tr.querySelectorAll(CONTROLS)].indexOf(el), inForm: !!el.closest('.tag-form') };
  if (el.tagName === 'INPUT' && el.type === 'text') { f.start = el.selectionStart; f.end = el.selectionEnd; }
  return f;
}
function restoreFocus(table, f) {
  if (!f || f.i < 0) return;
  const tr = table.querySelector(`tr[data-run="${CSS.escape(f.run)}"]`);
  if (!tr) return;
  // the form was closed (Cancel, Save): back to the row's Edit tags button
  const el = f.inForm && !tr.querySelector('.tag-form') ? tr.querySelector('button.edit-tags') : tr.querySelectorAll(CONTROLS)[f.i];
  if (!el) return;
  el.focus({ preventScroll: true });
  if (f.start != null && el.setSelectionRange) el.setSelectionRange(f.start, f.end);
}

// report.md: look up the file list when a row is first seen.
function watchRows(body) {
  if (observer) observer.disconnect();
  const pending = [...body.querySelectorAll('tr[data-run]')].filter((tr) => !hasReport.has(tr.dataset.run));
  if (!pending.length) return;
  if (!('IntersectionObserver' in window)) { pending.forEach((tr) => lookUpReport(tr.dataset.run)); return; }
  observer = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      observer.unobserve(e.target);
      lookUpReport(e.target.dataset.run);
    }
  }, { rootMargin: '200px 0px' });
  pending.forEach((tr) => observer.observe(tr));
}

function lookUpReport(id) {
  const runs = store.runs;
  getDetail(id).then((d) => {
    if (runs !== store.runs) return;
    hasReport.set(id, (d.files || []).some((f) => f.name === 'report.md'));
    fillReport(id);
  }, (err) => {
    if (runs !== store.runs) return;
    fillReport(id, err);
  });
}

function fillReport(id, err) {
  const slot = document.querySelector(`#runs-table tr[data-run="${CSS.escape(id)}"] .report-slot`);
  if (slot) clear(slot, reportLink(id, err));
}

function reportLink(id, err) {
  const known = hasReport.get(id);
  if (known === true) return h('a', { href: api.fileUrl(id, 'report.md'), target: '_blank', rel: 'noopener', text: 'report.md' });
  if (known === false) return h('span', { class: 'muted no-report', title: 'This run did not keep a report.md', text: 'no report.md' });
  return h('span', { class: 'muted pending-report', 'aria-disabled': 'true',
    title: err ? 'Cannot tell whether this run kept a report.md: ' + err.message : 'Checking whether this run kept a report.md…',
    text: 'report.md' });
}

function runRow(r) {
  const cb = h('input', { type: 'checkbox', 'aria-label': 'Select ' + r.label, checked: selected.has(r.run_id) });
  cb.addEventListener('change', () => { if (cb.checked) selected.add(r.run_id); else selected.delete(r.run_id); rerender(); });
  const flags = [];
  for (const st of r.failed_stages) flags.push(h('span', { class: 'flag', title: 'Values of this stage are left out', text: 'failed: ' + st }));
  if (!r.catalog_known) flags.push(h('span', { class: 'flag', title: 'No metric table for this vmbench version: values are stored but not judged', text: 'no metric table' }));
  if (r.tags.exclude) flags.push(h('span', { class: 'flag', title: 'Stages left out by the exclude tag', text: 'excluded: ' + r.tags.exclude }));
  const actions = h('div', { class: 'actions' },
    h('span', { class: 'report-slot' }, reportLink(r.run_id)),
    h('button', { type: 'button', class: 'small edit-tags', text: 'Edit tags', 'aria-label': 'Edit tags of ' + r.label,
      onclick: () => { openForm(r); rerender(); focusTagForm(r.run_id); } }),
    h('button', { type: 'button', class: 'small', text: 'Delete', 'aria-label': 'Delete ' + r.label, onclick: () => deleteRun(r) }));
  return h('tr', { 'data-run': r.run_id },
    h('td', {}, cb),
    h('td', { class: 'left' }, h('div', { text: r.label }), h('div', { class: 'muted mono', text: r.folder }), h('div', { class: 'muted mono', text: r.run_id })),
    h('td', { class: 'left' }, h('a', { href: '?view=compare&base=' + encodeURIComponent('group:' + r.group), text: r.group, title: 'Compare from this group',
      onclick: (e) => { e.preventDefault(); setState({ view: 'compare', base: 'group:' + r.group, other: [] }); } })),
    h('td', { class: 'left', text: archName(r.arch) }),
    h('td', { text: r.nproc == null ? '—' : String(r.nproc) }),
    h('td', { text: r.ram_gb == null ? '—' : String(r.ram_gb) }),
    h('td', { class: 'left wrap' }, h('div', { text: r.cpu_model || '—' })),
    h('td', { class: 'left', text: r.vmbench_version }),
    h('td', { class: 'left', text: shortDate(r.start_utc) }),
    h('td', { text: String(r.values) }),
    h('td', { class: 'left wrap' }, h('div', {}, flags.length ? flags : h('span', { class: 'muted', text: '—' }))),
    h('td', { class: 'tags' }, h('div', { class: 'tagbox' }, drafts.has(r.run_id) ? tagForm(r) : tagChips(r))),
    h('td', {}, actions));
}

function tagChips(r) {
  const e = Object.entries(r.tags);
  if (!e.length) return h('span', { class: 'muted', text: 'no tags' });
  return e.map(([k, v]) => h('span', { class: 'chip', title: k + '=' + v }, h('b', { text: k }), '=' + v));
}

function focusTagForm(id) {
  const el = document.querySelector(`tr[data-run="${CSS.escape(id)}"] .tag-form input`);
  if (el) el.focus();
}

const KEY_RE = /^[A-Za-z0-9_.-]{1,64}$/;

function openForm(r) {
  if (!drafts.has(r.run_id)) drafts.set(r.run_id, { rows: Object.entries(r.tags).map(([k, v]) => ({ k, v })), msg: '', saving: false });
}

/** The form is drawn from the draft; every keystroke goes back into the draft. */
function tagForm(r) {
  const d = drafts.get(r.run_id);
  const list = h('div', { class: 'rows' });
  const drawRow = (item) => {
    const key = h('input', { type: 'text', value: item.k, 'aria-label': 'Tag name', placeholder: 'name', spellcheck: 'false',
      oninput: () => { item.k = key.value; } });
    const val = h('input', { type: 'text', value: item.v, 'aria-label': 'Tag value', placeholder: 'value (empty removes)',
      oninput: () => { item.v = val.value; } });
    const row = h('div', { class: 'row' }, key, val,
      h('button', { type: 'button', class: 'small', text: '×', 'aria-label': 'Remove tag ' + (item.k || '(new)'),
        onclick: () => { d.rows.splice(d.rows.indexOf(item), 1); row.remove(); } }));
    list.append(row);
    return key;
  };
  d.rows.forEach(drawRow);
  const msg = h('p', { class: 'error', role: 'alert', hidden: !d.msg, text: d.msg });
  const save = h('button', { type: 'submit', class: 'small primary', text: d.saving ? 'Saving…' : 'Save', disabled: d.saving });
  const form = h('form', { class: 'tag-form', 'aria-label': 'Tags of ' + r.label }, list, msg,
    h('div', { class: 'buttons' },
      h('button', { type: 'button', class: 'small', text: '+ tag', onclick: () => { const item = { k: '', v: '' }; d.rows.push(item); drawRow(item).focus(); } }),
      save,
      h('button', { type: 'button', class: 'small', text: 'Cancel', onclick: () => { drafts.delete(r.run_id); rerender(); } })),
    h('p', { class: 'muted', text: 'group changes the run\'s group; exclude is a comma list of stages left out.' }));
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (d.saving) return;
    const cur = store.runById.get(r.run_id) || r;
    const next = {};
    for (const item of d.rows) {
      const k = item.k.trim(), v = item.v.trim();
      if (!k && !v) continue;
      if (!KEY_RE.test(k)) { showMsg(`Tag name "${k}": 1-64 of letters, digits, _ - .`); return; }
      if (v.length > 1024) { showMsg(`Tag ${k}: at most 1024 characters`); return; }
      next[k] = v;
    }
    const patch = {};
    for (const [k, v] of Object.entries(next)) if (cur.tags[k] !== v) patch[k] = v;
    for (const k of Object.keys(cur.tags)) if (!(k in next)) patch[k] = '';
    if (!Object.keys(patch).length) { drafts.delete(r.run_id); rerender(); return; }
    d.saving = true;
    d.msg = '';
    rerender();
    try {
      await api.patchTags(r.run_id, patch);
      drafts.delete(r.run_id);
      await onChanged();
    } catch (err) {
      d.saving = false;
      showMsg(err.message);
    }
  });
  // Through the draft and a re-render: the form on screen may be a newer copy than this one.
  function showMsg(t) { d.msg = t; if (drafts.get(r.run_id) === d) rerender(); }
  return form;
}

async function deleteRun(r) {
  const ok = await confirmDialog(
    `Delete run ${r.label} (${r.run_id}, folder ${r.folder})? This removes the run and its values (and its kept files) from the database.`,
    'Importing the same folder again brings the values back, but tags edited here or given by a registry are not restored unless they are given again (registry, benchlens-tags.json in the folder, or editing here).');
  if (!ok) return;
  const status = document.getElementById('status');
  try {
    await api.deleteRun(r.run_id);
    selected.delete(r.run_id);
    drafts.delete(r.run_id);
    clear(status, stateBlock('empty', `Deleted ${r.label}.`));
    await onChanged();
  } catch (err) {
    clear(status, errorBlock(err, 'Cannot delete ' + r.label));
  }
}
