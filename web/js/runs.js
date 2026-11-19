// Runs view: every run with search and filters, tags (inline editing via PATCH), delete, report.md link.

import { api } from './api.js';
import { store, sortedGroups } from './data.js';
import { setState } from './state.js';
import { h, clear, stateBlock, errorBlock, confirmDialog } from './ui.js';
import { archName, shortDate } from './format.js';

const selected = new Set();
const editing = new Set();
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
  const runs = store.runs.filter((r) => matches(r, s));
  document.getElementById('runs-count').textContent = `${runs.length} of ${store.runs.length} runs` + (selected.size ? ` · ${selected.size} selected` : '');
  const hw = document.getElementById('runs-hw');
  hw.disabled = !selected.size;
  hw.textContent = selected.size ? `Hardware of selected (${selected.size})` : 'Hardware of selected';

  const table = document.getElementById('runs-table');
  const cols = ['', 'Run', 'Group', 'Arch', 'vCPU', 'RAM GB', 'CPU', 'vmbench', 'Started', 'Values', 'Flags', 'Tags', 'Actions'];
  const head = h('tr', {}, ...cols.map((c, i) => h('th', { scope: 'col', class: i === 0 ? '' : 'left' }, i === 0 ? h('span', { class: 'sr-only', text: 'Select' }) : c)));
  const body = h('tbody');
  for (const r of runs) body.append(runRow(r, s));
  if (!runs.length) body.append(h('tr', {}, h('td', { colspan: String(cols.length), class: 'left muted', text: 'No run matches these filters.' })));
  clear(table, h('thead', {}, head), body);
}

function runRow(r, s) {
  const cb = h('input', { type: 'checkbox', 'aria-label': 'Select ' + r.label, checked: selected.has(r.run_id) });
  cb.addEventListener('change', () => { if (cb.checked) selected.add(r.run_id); else selected.delete(r.run_id); renderRuns(s); });
  const flags = [];
  for (const st of r.failed_stages) flags.push(h('span', { class: 'flag', title: 'Values of this stage are left out', text: 'failed: ' + st }));
  if (!r.catalog_known) flags.push(h('span', { class: 'flag', title: 'No metric table for this vmbench version: values are stored but not judged', text: 'no metric table' }));
  if (r.tags.exclude) flags.push(h('span', { class: 'flag', title: 'Stages left out by the exclude tag', text: 'excluded: ' + r.tags.exclude }));
  const actions = h('div', { class: 'actions' },
    h('a', { href: api.fileUrl(r.run_id, 'report.md'), target: '_blank', rel: 'noopener', text: 'report.md' }),
    h('button', { type: 'button', class: 'small', text: 'Edit tags', 'aria-label': 'Edit tags of ' + r.label,
      onclick: () => { editing.add(r.run_id); renderRuns(s); focusTagForm(r.run_id); } }),
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
    h('td', { class: 'tags' }, h('div', { class: 'tagbox' }, editing.has(r.run_id) ? tagForm(r, s) : tagChips(r))),
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

function tagForm(r, s) {
  const list = h('div', { class: 'rows' });
  const addRow = (k = '', v = '') => {
    const key = h('input', { type: 'text', value: k, 'aria-label': 'Tag name', placeholder: 'name', spellcheck: 'false' });
    const val = h('input', { type: 'text', value: v, 'aria-label': 'Tag value', placeholder: 'value (empty removes)' });
    const row = h('div', { class: 'row' }, key, val,
      h('button', { type: 'button', class: 'small', text: '×', 'aria-label': 'Remove tag ' + k, onclick: () => { row.remove(); } }));
    list.append(row);
    return key;
  };
  for (const [k, v] of Object.entries(r.tags)) addRow(k, v);
  const msg = h('p', { class: 'error', role: 'alert', hidden: true });
  const save = h('button', { type: 'submit', class: 'small primary', text: 'Save' });
  const form = h('form', { class: 'tag-form', 'aria-label': 'Tags of ' + r.label }, list, msg,
    h('div', { class: 'buttons' },
      h('button', { type: 'button', class: 'small', text: '+ tag', onclick: () => addRow().focus() }),
      save,
      h('button', { type: 'button', class: 'small', text: 'Cancel', onclick: () => { editing.delete(r.run_id); renderRuns(s); } })),
    h('p', { class: 'muted', text: 'group changes the run\'s group; exclude is a comma list of stages left out.' }));
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const next = {};
    for (const row of list.querySelectorAll('.row')) {
      const [k, v] = [...row.querySelectorAll('input')].map((i) => i.value.trim());
      if (!k && !v) continue;
      if (!KEY_RE.test(k)) { showMsg(`Tag name "${k}": 1-64 of letters, digits, _ - .`); return; }
      if (v.length > 1024) { showMsg(`Tag ${k}: at most 1024 characters`); return; }
      next[k] = v;
    }
    const patch = {};
    for (const [k, v] of Object.entries(next)) if (r.tags[k] !== v) patch[k] = v;
    for (const k of Object.keys(r.tags)) if (!(k in next)) patch[k] = '';
    if (!Object.keys(patch).length) { editing.delete(r.run_id); renderRuns(s); return; }
    save.disabled = true;
    save.textContent = 'Saving…';
    try {
      await api.patchTags(r.run_id, patch);
      editing.delete(r.run_id);
      await onChanged();
    } catch (err) {
      showMsg(err.message);
      save.disabled = false;
      save.textContent = 'Save';
    }
  });
  function showMsg(t) { msg.textContent = t; msg.hidden = false; }
  return form;
}

async function deleteRun(r) {
  const ok = await confirmDialog(`Delete run ${r.label} (${r.run_id}, folder ${r.folder})? Its values and kept files are removed from the database. Importing the folder again brings it back.`);
  if (!ok) return;
  const status = document.getElementById('status');
  try {
    await api.deleteRun(r.run_id);
    selected.delete(r.run_id);
    editing.delete(r.run_id);
    clear(status, stateBlock('empty', `Deleted ${r.label}.`));
    await onChanged();
  } catch (err) {
    clear(status, errorBlock(err, 'Cannot delete ' + r.label));
  }
}
