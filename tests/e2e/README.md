# End-to-end test

Playwright tests against the real `benchlens serve` on the fixtures. CI runs them on every PR (job `e2e` in
`.github/workflows/ci.yml`).

```bash
scripts/e2e.sh                          # whole suite, about 20 s after the release build
scripts/e2e.sh --grep heatmap           # extra arguments go to `playwright test`
BENCHLENS_E2E_SKIP_BUILD=1 scripts/e2e.sh   # reuse target/release/benchlens
```

`scripts/e2e.sh` builds the release binary, copies `tests/fixtures` and writes a placeholder `report.md` into
every run folder but `x86-std-r2-20261119-103253` (the fixtures keep none; the Runs view's report.md link is
checked both ways), imports the copies with `examples/poc-2026-11/registry.tsv` and starts three
`benchlens serve --listen 127.0.0.1:0` (the OS picks free ports; the script reads them from the servers' first
log lines and waits for `/api/health`):

| Server | Env for the tests | Database | Access |
|---|---|---|---|
| open | `BENCHLENS_URL` (Playwright's `baseURL`) | every fixture | loopback, no token: reads and writes open |
| token | `BENCHLENS_TOKEN_URL` | every fixture but x86-std-r2 | `BENCHLENS_TOKEN` (random, `BENCHLENS_E2E_TOKEN`): writes need it |
| private | `BENCHLENS_PRIVATE_URL` | same as token | token and `BENCHLENS_READ_PUBLIC=false`: reads need it too |

Then it runs `npx playwright test` here, and, when no extra arguments were given, `tests/export/run.sh` (the
static HTML export opened from disk with the network off, checked against `benchlens compare --json`). It always
stops the servers (SIGTERM) and removes the temporary directory. It runs `npm ci` itself when `node_modules` is
missing. Screenshots of every view, and traces and screenshots of failed tests, land in `tests/e2e/test-results/`
(CI uploads that folder when the job fails; open a trace with
`npx playwright show-trace test-results/<test>/trace.zip`).

## What it checks

Nothing is hard-coded: the comparisons and expected numbers are read from the server at run time, the run count
from the fixture folders.

| File | |
|---|---|
| `tests/api.spec.js` | every read endpoint of `docs/api.md` has the documented keys and types (no key missing, none undocumented); run filters; files by name with their byte counts; tallies equal the rows; `section=` keeps tallies; 404 for unknown runs / groups / files / tables, 400 for unknown query keys, a repeated single key, compare without `other`, metrics without `version`, `group:` as a run id |
| `tests/views.spec.js` | compare (key and all), metric (also a sysbench row across architectures), runs and hardware at 1280 and 400 px, light and dark: data rendered, no console error, no page error, no failed request, no sideways scroll and nothing past the right edge outside its own scroll box; a screenshot of each |
| `tests/compare.spec.js` | for two comparisons picked from the data (the two biggest groups on different architectures; the two fullest runs of the biggest group): tally table (totals and per section) and takeaway, every heatmap cell's verdict class, percentage text (`format.js`'s rule: `+12%`, `−3.4%`, `·` when measured once) and screen-reader verdict, the dot plot's number of dots in total and per side against `/api/values`, the statistics table against `/api/compare` |
| `tests/state.spec.js` | selections made in the UI (compare sides, section, baseline, single runs, metric, runs filters, hardware runs) are in the URL and a reload restores them; back button |
| `tests/runs.spec.js` | an open tag form keeps its draft, focus and caret across a re-render, a ticked checkbox and a filter change, Cancel saves nothing; report.md is linked exactly for runs whose `kept_files` has it (equal to RunDetail's file names, no RunDetail request per row; the link works), "no report.md" otherwise, and opens as plain text in a dialog (keyboard, Escape returns focus); the delete dialog's wording, Cancel deletes nothing |
| `tests/export.spec.js` | the Export button downloads the report of the shown sides (inlined data names them); `/api/export.html` headers (attachment, CSP) |
| `tests/auth.spec.js` | token server: PATCH, DELETE and upload without or with a wrong token are 401 with `WWW-Authenticate`; upload through the dialog asks for the token, says a wrong one was refused, imports with the tag given; the same archive again through the API is `already_present`. Private server: every `/api` read but health is 401 without the token; the page asks for it, then renders, and the Export button sends it; report.md opens from the Runs view with the token |
| `tests/split.spec.js` | compare picker, split a group by tag (group, tag and values chosen from `/api/runs`, by keyboard): one `group:NAME@KEY=VALUE` side per value with the API's label, in the URL, dropped again by unticking; first value as the baseline; nothing cut off at 400 px |
| `tests/tags.spec.js` | add and remove a tag in the Runs view, see it in `/api/runs` and as a filter; a bad tag name is refused in the browser; tags are put back at the end |

The tests run one after the other (`workers: 1`) because the tag and upload tests write to the databases.

## Running without root (no system libraries for Chromium)

Chromium needs a few system libraries (`npx playwright install-deps chromium` lists and installs them with
root). On a machine without root, download the missing packages and unpack them into a folder of your own,
then point `LD_LIBRARY_PATH` at it. On Ubuntu 24.04 (WSL) only `libnss3` and `libnspr4` were missing:

```bash
cd tests/e2e && npm ci && npx playwright install chromium   # browser into ~/.cache/ms-playwright
mkdir -p ~/pw-libs/debs && cd ~/pw-libs/debs
apt-get download libnss3 libnspr4                            # no root needed
for d in *.deb; do dpkg -x "$d" ~/pw-libs/root; done
export LD_LIBRARY_PATH=~/pw-libs/root/usr/lib/x86_64-linux-gnu
cd - && scripts/e2e.sh
```

If Chromium still does not start, `ldd ~/.cache/ms-playwright/chromium_headless_shell-*/chrome-*/chrome-headless-shell | grep 'not found'`
names the next library to fetch the same way.

## Updating Playwright

Change the version in `package.json`, run `npm install` here and commit `package-lock.json`; the CI browser
cache is keyed on the lock file.
