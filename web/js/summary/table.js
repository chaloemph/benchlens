// The "Show as table" fallback every Summary chart has: the same numbers as the chart, as text.

import { h } from '../ui.js';

/**
 * `<details>` with a table: `columns` = header texts, `rows` = arrays of cell values (shown with textContent),
 * `label` names the table for screen readers.
 */
export function tableFallback({ label, columns, rows }) {
  const head = h('tr', {}, ...columns.map((c) => h('th', { scope: 'col', text: c })));
  const body = rows.map((r) => h('tr', {}, ...r.map((v, i) => h(i ? 'td' : 'th', { scope: i ? null : 'row', text: v == null ? '' : String(v) }))));
  return h('details', { class: 'sum-table' },
    h('summary', { text: 'Show as table' }),
    h('div', { class: 'scroll', tabindex: '0', role: 'region', 'aria-label': label },
      h('table', { class: 'data' }, h('thead', {}, head), h('tbody', {}, ...body))));
}
