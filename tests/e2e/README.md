# End-to-end test

Playwright tests against the real `benchlens serve` on the fixtures. CI runs them on every PR (job `e2e` in
`.github/workflows/ci.yml`).

```bash
scripts/e2e.sh                          # whole suite, about 20 s after the release build
scripts/e2e.sh --grep heatmap           # extra arguments go to `playwright test`
BENCHLENS_E2E_SKIP_BUILD=1 scripts/e2e.sh   # reuse target/release/benchlens
```

`scripts/e2e.sh` builds the release binary, imports `tests/fixtures/v0.2.1` and `v0.2.3` with
`examples/poc-2026-11/registry.tsv` into a temporary database, starts `benchlens serve --listen 127.0.0.1:0`
(the OS picks a free port; the script reads it from the server's first log line), waits for `/api/health`,
runs `npx playwright test` here with `BENCHLENS_URL` set, and always stops the server and removes the
temporary directory. It runs `npm ci` itself when `node_modules` is missing. Screenshots of every view, and
traces and screenshots of failed tests, land in `tests/e2e/test-results/` (CI uploads that folder when the job
fails; open a trace with `npx playwright show-trace test-results/<test>/trace.zip`).

## What it checks

Nothing is hard-coded: the comparisons and expected numbers are read from the server at run time, the run count
from the fixture folders.

| File | |
|---|---|
| `tests/api.spec.js` | every read endpoint of `docs/api.md` has the documented keys and types (no key missing, none undocumented); run filters; files by name with their byte counts; tallies equal the rows; `section=` keeps tallies; 404 for unknown runs / groups / files / tables, 400 for unknown query keys, a repeated single key, compare without `other`, metrics without `version`, `group:` as a run id |
| `tests/views.spec.js` | compare (key and all), metric (also a sysbench row across architectures), runs and hardware at 1280 and 400 px, light and dark: data rendered, no console error, no page error, no failed request, no sideways scroll and nothing cut off at the right edge (body has `overflow-x: hidden`, so the test also looks for elements past the viewport outside their own scroll box); a screenshot of each |
| `tests/compare.spec.js` | for two comparisons picked from the data (the two biggest groups on different architectures; the two fullest runs of the biggest group): tally table (totals and per section) and takeaway, every heatmap cell's verdict class, percentage text (`format.js`'s rule: `+12%`, `−3.4%`, `·` when measured once) and screen-reader verdict, the dot plot's number of dots in total and per side against `/api/values`, the statistics table against `/api/compare` |
| `tests/state.spec.js` | selections made in the UI (compare sides, section, baseline, single runs, metric, runs filters, hardware runs) are in the URL and a reload restores them; back button |
| `tests/tags.spec.js` | add and remove a tag in the Runs view, see it in `/api/runs` and as a filter; a bad tag name is refused in the browser; tags are put back at the end |

The tests run one after the other (`workers: 1`) because the tag test writes to the shared database.

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
