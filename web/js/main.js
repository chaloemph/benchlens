// The benchlens UI shell: loads runs and groups through `api`, then renders the view named in the URL.
// Used by the live UI (app.js, data from the server) and by the static HTML export (export/offline.js, data
// inlined in the file and compared with the WASM build of the verdict rule), so both render the same way.

import { store, loadAll, defaultComparison } from './data.js';
import { readState, setState, hrefFor } from './state.js';
import { clear, stateBlock, errorBlock, emptyRunsBlock, hideTip } from './ui.js';
import { renderCompare, dropComparisonCache } from './compare.js';
import { renderSummary } from './summary/index.js';
import { renderMetric, dropValueCache } from './detail.js';
import { initRuns, renderRuns } from './runs.js';
import { initHardware, renderHardware, dropDetailCache } from './hardware.js';

const ALL_VIEWS = ['summary', 'compare', 'metric', 'runs', 'hardware'];
const TITLES = { summary: 'Summary', compare: 'Compare', metric: 'Metric', runs: 'Runs', hardware: 'Hardware' };

/**
 * Start the UI.
 * - `views`: the views offered (default all); a link to another view shows the comparison.
 * - `firstComparison`: `() => ({ base, others })` used when the URL names no sides (default: first quick pick).
 * - `title`: start of the document title (default "benchlens").
 * - `setup`: called once with `reload` (reloads runs and groups and renders again), e.g. for the upload
 *   dialog of the live UI; without it the header's Upload button is hidden.
 */
export function start({ views = ALL_VIEWS, firstComparison = defaultComparison, title = 'benchlens', setup = null } = {}) {
  let lastView = null;

  function render() {
    hideTip();
    const s = readState();
    if (!views.includes(s.view)) { setState({ view: 'compare' }, { replace: true }); return; }
    const status = document.getElementById('status');
    if (s.view !== lastView) { clear(status); if (lastView) window.scrollTo(0, 0); lastView = s.view; }
    for (const a of document.querySelectorAll('nav a[data-view]')) {
      a.href = hrefFor({ view: a.dataset.view });
      if (a.dataset.view === s.view) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
    }
    for (const v of ALL_VIEWS) document.getElementById('view-' + v).hidden = v !== s.view || !store.runs.length;
    document.title = title + ' · ' + TITLES[s.view];
    if (!store.loaded) return;
    if (!store.runs.length) { clear(status, emptyRunsBlock()); return; }

    if ((s.view === 'summary' || s.view === 'compare' || s.view === 'metric') && !s.base) {
      const d = firstComparison();
      if (d) { setState({ base: d.base, other: d.others }, { replace: true }); return; }
    }
    if (s.view === 'summary') renderSummary(s);
    else if (s.view === 'compare') renderCompare(s);
    else if (s.view === 'metric') renderMetric(s);
    else if (s.view === 'runs') renderRuns(s);
    else renderHardware(s);
  }

  async function reload() {
    dropComparisonCache();
    dropValueCache();
    dropDetailCache();
    await loadAll();
    render();
  }

  async function boot() {
    const status = document.getElementById('status');
    render();
    clear(status, stateBlock('loading', 'Loading runs…'));
    try {
      await loadAll();
    } catch (err) {
      clear(status, errorBlock(err, 'Cannot load the runs'));
      return;
    }
    clear(status);
    render();
  }

  for (const a of document.querySelectorAll('nav a[data-view]')) {
    if (!views.includes(a.dataset.view)) { a.closest('li').hidden = true; continue; }
    a.addEventListener('click', (e) => {
      if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
      e.preventDefault();
      setState({ view: a.dataset.view });
      document.getElementById('main').focus({ preventScroll: true });
    });
  }
  window.addEventListener('popstate', render);
  window.addEventListener('statechange', render);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') hideTip(); });
  if (views.includes('runs')) initRuns(reload);
  initHardware();
  const upload = document.getElementById('upload-open');
  if (setup) setup(reload); else if (upload) upload.hidden = true;
  return boot();
}
