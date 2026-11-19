// benchlens web UI entry point: loads runs and groups, then renders the view named in the URL.

import { store, loadAll, defaultComparison } from './data.js';
import { readState, setState, hrefFor } from './state.js';
import { clear, stateBlock, errorBlock, emptyRunsBlock, hideTip } from './ui.js';
import { renderCompare, dropComparisonCache } from './compare.js';
import { renderMetric, dropValueCache } from './detail.js';
import { initRuns, renderRuns } from './runs.js';
import { initHardware, renderHardware, dropDetailCache } from './hardware.js';

const VIEWS = ['compare', 'metric', 'runs', 'hardware'];
const TITLES = { compare: 'Compare', metric: 'Metric', runs: 'Runs', hardware: 'Hardware' };
let lastView = null;

function render() {
  hideTip();
  const s = readState();
  const status = document.getElementById('status');
  if (s.view !== lastView) { clear(status); if (lastView) window.scrollTo(0, 0); lastView = s.view; }
  for (const a of document.querySelectorAll('nav a[data-view]')) {
    a.href = hrefFor({ view: a.dataset.view });
    if (a.dataset.view === s.view) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
  }
  for (const v of VIEWS) document.getElementById('view-' + v).hidden = v !== s.view || !store.runs.length;
  document.title = 'benchlens · ' + TITLES[s.view];
  if (!store.loaded) return;
  if (!store.runs.length) { clear(status, emptyRunsBlock()); return; }

  if ((s.view === 'compare' || s.view === 'metric') && !s.base) {
    const d = defaultComparison();
    if (d) { setState({ base: d.base, other: d.others }, { replace: true }); return; }
  }
  if (s.view === 'compare') renderCompare(s);
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
initRuns(reload);
initHardware();
boot();
