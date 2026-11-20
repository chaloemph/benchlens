// Upload dialog: send a .tar / .tar.gz of vmbench result folders to POST /api/runs with optional tags, then
// list what happened to each folder. The token prompt (api.js) handles 401. Values go in with textContent only.

import { api } from './api.js';
import { h, clear } from './ui.js';

const KEY_RE = /^[A-Za-z0-9_.-]{1,64}$/;   // the server's tag key rule
const OUTCOME = { imported: 'imported', already_present: 'already here', failed: 'failed' };

let dlg = null;
let onDone = async () => {};

/** Wire the header's Upload button; `fn` reloads runs and groups after an upload that changed something. */
export function initUpload(fn) {
  onDone = fn;
  const open = document.getElementById('upload-open');
  if (open) open.addEventListener('click', () => openDialog());
}

function tagRow(key = '', value = '') {
  const row = h('div', { class: 'row' });
  const k = h('input', { type: 'text', class: 'tag-key', 'aria-label': 'Tag key', placeholder: 'key', value: key, autocomplete: 'off' });
  const v = h('input', { type: 'text', class: 'tag-value', 'aria-label': 'Tag value', placeholder: 'value', value, autocomplete: 'off' });
  const rm = h('button', { type: 'button', class: 'small', 'aria-label': 'Remove this tag', text: '✕', onclick: () => row.remove() });
  row.append(k, v, rm);
  return row;
}

function build() {
  const file = h('input', { type: 'file', id: 'upload-file', accept: '.tar,.gz,.tgz,application/gzip,application/x-tar', required: true });
  const tags = h('div', { class: 'upload-tags', id: 'upload-tags' });
  const msg = h('p', { class: 'error', id: 'upload-error', hidden: true, role: 'alert' });
  const busy = h('p', { class: 'muted', id: 'upload-busy', hidden: true, role: 'status', text: 'Uploading and importing…' });
  const results = h('ul', { class: 'upload-results', id: 'upload-results', 'aria-live': 'polite' });
  const submit = h('button', { type: 'submit', class: 'primary', id: 'upload-submit', text: 'Upload' });
  const close = h('button', { type: 'button', text: 'Close', onclick: () => dlg.close() });
  const form = h('form', { novalidate: true },
    h('h2', { id: 'upload-title', text: 'Upload results' }),
    h('p', { class: 'muted', text: 'A .tar or .tar.gz of one or more vmbench result folders (as vmbench leaves them). Runs already here are not added twice; tags are set on every run in the archive.' }),
    h('div', { class: 'field' }, h('label', { for: 'upload-file', text: 'Archive' }), file),
    h('fieldset', { class: 'upload-tagset' },
      h('legend', { text: 'Tags (optional)' }),
      h('p', { class: 'muted', id: 'upload-registry-hint', text: 'If the server has a registry, its tags apply too; tags given here win over it.' }),
      tags,
      h('button', { type: 'button', class: 'small', id: 'upload-add-tag', text: 'Add tag', onclick: () => {
        const r = tagRow();
        tags.append(r);
        r.querySelector('input').focus();
      } })),
    msg, busy, results,
    h('div', { class: 'buttons' }, submit, close));
  form.addEventListener('submit', (e) => { e.preventDefault(); send(form); });
  const d = h('dialog', { id: 'upload-dialog', class: 'upload', 'aria-labelledby': 'upload-title' }, form);
  document.body.append(d);
  return d;
}

function openDialog() {
  if (!dlg) dlg = build();
  const msg = dlg.querySelector('#upload-error');
  msg.hidden = true;
  if (!dlg.open) dlg.showModal();
  dlg.querySelector('#upload-file').focus();
}

function showError(text, detail) {
  const msg = dlg.querySelector('#upload-error');
  clear(msg, text, detail ? h('div', { class: 'muted', text: detail }) : null);
  msg.hidden = false;
}

/** Tags from the rows: [[key, value], ...]; throws a message for a bad row. Empty rows are skipped. */
function readTags() {
  const out = [];
  for (const row of dlg.querySelectorAll('#upload-tags .row')) {
    const k = row.querySelector('.tag-key').value.trim();
    const v = row.querySelector('.tag-value').value.trim();
    if (!k && !v) continue;
    if (!KEY_RE.test(k)) throw new Error(`Tag key "${k}": use 1-64 letters, digits, _ - or .`);
    out.push([k, v]);
  }
  return out;
}

function renderResults(reports, fileName) {
  const list = dlg.querySelector('#upload-results');
  const items = reports.map((r) => h('li', { class: 'upload-result ' + r.outcome },
    h('span', { class: 'outcome', text: OUTCOME[r.outcome] || r.outcome }), ' ',
    h('strong', { text: r.folder }),
    r.run_id ? h('span', { class: 'mono', text: ' ' + r.run_id }) : null,
    r.message ? h('div', { class: 'muted', text: r.message }) : null));
  clear(list, h('li', { class: 'upload-file-name muted', text: fileName + ':' }), ...items);
}

async function send(form) {
  const fileInput = form.querySelector('#upload-file');
  const submit = form.querySelector('#upload-submit');
  const busy = form.querySelector('#upload-busy');
  form.querySelector('#upload-error').hidden = true;
  const file = fileInput.files && fileInput.files[0];
  if (!file) { showError('Choose a .tar or .tar.gz file first.'); fileInput.focus(); return; }
  let tags;
  try { tags = readTags(); } catch (e) { showError(e.message); return; }
  submit.disabled = true;
  busy.hidden = false;
  try {
    const reports = await api.upload(file, tags);
    renderResults(reports, file.name);
    fileInput.value = '';
    if (reports.some((r) => r.outcome === 'imported' || r.outcome === 'already_present')) await onDone();
  } catch (err) {
    showError(err.message || String(err), err.detail);
  } finally {
    submit.disabled = false;
    busy.hidden = true;
  }
}
