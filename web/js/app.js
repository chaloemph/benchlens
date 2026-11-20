// benchlens web UI entry point (served by `benchlens serve`): every view, data from the JSON API.

import { start } from './main.js';
import { clear } from './ui.js';
import { initUpload } from './upload.js';

start({ setup: (reload) => initUpload(async () => { clear(document.getElementById('status')); await reload(); }) });
