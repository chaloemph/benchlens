// Small DOM helpers. Every data value goes in with textContent, never innerHTML.

/** h('td', {class: 'x', title: '...'}, 'text', node, ...) */
export function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'text') el.textContent = v;
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v);
    else if (k in el && typeof v !== 'string') el[k] = v;
    else el.setAttribute(k, v === true ? '' : String(v));
  }
  append(el, children);
  return el;
}

function append(el, children) {
  for (const c of children.flat()) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

export function clear(el, ...children) {
  el.replaceChildren();
  append(el, children);
  return el;
}

/** A loading / empty / error block. */
export function stateBlock(kind, ...children) {
  const el = h('div', { class: 'state ' + kind + (kind === 'loading' ? ' loading' : '') }, ...children);
  if (kind === 'error') el.setAttribute('role', 'alert');
  return el;
}

export function errorBlock(err, what) {
  const kids = [h('strong', { text: what ? what + ': ' : 'Error: ' }), err && err.message ? err.message : String(err)];
  if (err && err.detail) kids.push(h('div', { class: 'muted', text: err.detail }));
  return stateBlock('error', ...kids);
}

export function emptyRunsBlock() {
  return stateBlock('empty',
    h('strong', { text: 'No runs yet.' }), ' Import vmbench result folders, then reload: ',
    h('code', { text: 'benchlens import path/to/results' }),
    h('div', { class: 'muted' }, 'Folders, directories of folders and .tar / .tar.gz archives all work; ',
      h('code', { text: '--registry registry.tsv' }), ' tags them on the way in.'));
}

// Tooltip: one floating box, positioned near the pointer or the focused element.
const tip = () => document.getElementById('tip');
export function showTip(ev, lines) {
  const t = tip();
  t.replaceChildren();
  lines.forEach((line, i) => t.append(i ? h('div', { text: line }) : h('b', { text: line })));
  t.hidden = false;
  let x, y;
  if (ev && 'clientX' in ev && ev.clientX) { x = ev.clientX + 14; y = ev.clientY + 16; }
  else { const r = ev.target.getBoundingClientRect(); x = r.left; y = r.bottom + 6; }
  const w = t.offsetWidth, hh = t.offsetHeight;
  if (x + w > innerWidth - 8) x = Math.max(8, innerWidth - w - 8);
  if (y + hh > innerHeight - 8) y = Math.max(8, y - hh - 28);
  t.style.left = x + 'px';
  t.style.top = y + 'px';
}
export function hideTip() { tip().hidden = true; }

/** Ask for confirmation in the confirm dialog. */
export function confirmDialog(text) {
  const dlg = document.getElementById('confirm-dialog');
  document.getElementById('confirm-text').textContent = text;
  return new Promise((resolve) => {
    dlg.addEventListener('close', () => resolve(dlg.returnValue === 'ok'), { once: true });
    dlg.returnValue = '';
    dlg.showModal();
    dlg.querySelector('button[value="cancel"]').focus();
  });
}
