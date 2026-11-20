// HTTP client for the benchlens API v1 (docs/api.md). Errors carry the API's `error` message.
// A 401 opens the token prompt; the token is kept in sessionStorage and sent as `Authorization: Bearer`.

const TOKEN_KEY = 'benchlens-token';

export class ApiError extends Error {
  constructor(status, message, detail) {
    super(message);
    this.status = status;
    this.detail = detail || '';
  }
}

function getToken() {
  try { return sessionStorage.getItem(TOKEN_KEY) || ''; } catch { return ''; }
}
function setToken(t) {
  try { if (t) sessionStorage.setItem(TOKEN_KEY, t); else sessionStorage.removeItem(TOKEN_KEY); } catch { /* storage blocked */ }
}

/** Ask for a token in the dialog; resolves to the token, or '' when cancelled. Requests refused at the same
 *  time (the first page load fetches several things at once) share one dialog. */
let pendingAsk = null;
function askToken(message) {
  if (!pendingAsk) pendingAsk = showTokenDialog(message).finally(() => { pendingAsk = null; });
  return pendingAsk;
}

function showTokenDialog(message) {
  const dlg = document.getElementById('token-dialog');
  const input = document.getElementById('token-input');
  const err = document.getElementById('token-error');
  err.hidden = !message;
  err.textContent = message || '';
  input.value = '';
  return new Promise((resolve) => {
    dlg.addEventListener('close', () => {
      resolve(dlg.returnValue === 'ok' ? input.value.trim() : '');
    }, { once: true });
    dlg.returnValue = '';
    dlg.showModal();
    input.focus();
  });
}

async function errorOf(res) {
  let msg = `${res.status} ${res.statusText}`.trim(), detail = '';
  try {
    const body = await res.json();
    if (body && typeof body.error === 'string') { msg = body.error; detail = body.detail || ''; }
  } catch { /* not JSON */ }
  return new ApiError(res.status, msg, detail);
}

async function request(method, path, body) {
  for (let attempt = 0; attempt < 3; attempt++) {
    const headers = { Accept: 'application/json' };
    const token = getToken();
    if (token) headers.Authorization = 'Bearer ' + token;
    const form = body instanceof FormData;  // multipart: the browser sets Content-Type with the boundary
    if (body !== undefined && !form) headers['Content-Type'] = 'application/json';
    let res;
    try {
      res = await fetch(path, { method, headers, body: body === undefined ? undefined : form ? body : JSON.stringify(body) });
    } catch (e) {
      throw new ApiError(0, 'Cannot reach the benchlens server (' + e.message + ')');
    }
    if (res.status === 401) {
      const err = await errorOf(res);
      // another request may have stored a new token while this one was in flight: retry with it first
      if (token && getToken() !== token) continue;
      if (token) setToken('');
      const t = await askToken(token ? 'That token was refused: ' + err.message : '');
      if (!t) throw new ApiError(401, err.message || 'a token is needed');
      setToken(t);
      continue;
    }
    if (!res.ok) throw await errorOf(res);
    if (res.status === 204) return null;
    const type = res.headers.get('content-type') || '';
    return type.includes('json') ? res.json() : res.text();
  }
  throw new ApiError(401, 'a token is needed');
}

const enc = encodeURIComponent;

export const api = {
  health: () => request('GET', 'api/health'),
  runs: () => request('GET', 'api/runs'),
  run: (id) => request('GET', 'api/runs/' + enc(id)),
  groups: () => request('GET', 'api/groups'),
  compare: (base, others) => {
    const q = new URLSearchParams();
    q.append('base', base);
    for (const o of others) q.append('other', o);
    return request('GET', 'api/compare?' + q);
  },
  /** The static HTML report of a comparison (GET /api/export.html), as text; null where there is no server. */
  exportHtml: (base, others) => {
    const q = new URLSearchParams();
    q.append('base', base);
    for (const o of others) q.append('other', o);
    return request('GET', 'api/export.html?' + q);
  },
  values: (ref, id) => request('GET', 'api/values?' + new URLSearchParams({ ref, id })),
  metrics: (version) => request('GET', 'api/metrics?' + new URLSearchParams({ version })),
  fileUrl: (id, name) => 'api/runs/' + enc(id) + '/files/' + enc(name),
  patchTags: (id, tags) => request('PATCH', 'api/runs/' + enc(id) + '/tags', tags),
  deleteRun: (id) => request('DELETE', 'api/runs/' + enc(id)),
  /** POST /api/runs: `file` (a File) and tags as [key, value] pairs; answers ImportReport[]. */
  upload: (file, tags) => {
    const fd = new FormData();
    fd.append('file', file, file.name);
    for (const [k, v] of tags) fd.append('tag', k + '=' + v);
    return request('POST', 'api/runs', fd);
  },
};
