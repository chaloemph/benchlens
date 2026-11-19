# benchlens plan: M2 onwards, split for parallel agents

Status 2026-11-19: **M1 done** (commit 298860c): core library, CLI, SQLite store, golden tests against vmbench.
**T0 done** (PR #1). **Wave 1 done** (A, B, C1, E, F reviewed and integrated in `integration/wave1`; V in a
vmbench PR); reviews in `docs/reviews/wave1-*.md`, follow-ups in "Follow-ups from the Wave 1 review" below.
Background and decisions: `PLAN-dashboard.md` in the performance-benchmark workspace (Thai). This file is
the working plan for whoever builds the rest, person or agent.

## Ground rules for every task

1. **The verdict rule is vmbench's.** Do not change `crates/core/src/verdict.rs` or how `compare.rs` pools
   values unless vmbench changed first; `cargo test` (golden) must pass on every branch.
2. **One task, one branch, one worktree.** Branch `m2/<task-id>-<slug>`. Task agents commit on their branch and
   stop: they do not push or open PRs. Every branch goes through task **L** (lead review), which is the only
   one that opens PRs; the owner merges.
3. **Stay inside your files** (column "Owns" below). Need a change elsewhere? Ask in the PR, or make it the
   smallest possible and say so in the PR description.
4. **Real data only.** Develop and test against `tests/fixtures/` and the JSON the CLI prints
   (`benchlens compare ... --json`, `benchlens runs --json`). No invented numbers.
5. **No new runtime services.** Single binary, SQLite file. A dependency that needs a system library or a
   network service needs a note in the PR.
6. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all` before every push.
7. Untrusted input everywhere: archives, uploads, tags, query strings. Never build SQL from strings, never
   write outside the configured data folder, never follow links.

## Dependency graph

```
            T0 (lead, first, serial)
   rules crate split + API contract + server skeleton
                     │
   ┌────────┬────────┼─────────┬──────────┬──────────┐
   A        B        E         F          C1         V
 server   web UI    wasm   packaging    inbox     vmbench fix
   │        │        │                    │       (other repo)
   └──┬─────┴────────┤                    │
      C2 upload+auth │                    │
      (needs A)      D export (needs B,E) │
                     │                    │
              ───── M4 done ─────────────┘
                     │
            G Grafana       H docs / ecosystem
```

Wave 0: T0 only. Wave 1: A, B, E, F, C1, V in parallel. Wave 2: C2, D, and A+B integration. Wave 3: G, H.
After each wave: **L** (lead review) takes every finished branch of that wave, then the next wave starts
from what L integrated.

## T0 (lead, before anyone else starts): done in PR `m2/t0-foundation`

Small and serial, because every other task builds on it.

| Item | Detail |
|---|---|
| Split pure logic | New crate `crates/rules` (`benchlens-rules`): `verdict`, `catalog` parsing/display names, `stats()` and the per-metric judging from `compare.rs`, taking plain value rows. No rusqlite, tar or fs, so it builds for `wasm32-unknown-unknown`. `core` re-exports it; golden tests keep passing unchanged. |
| API contract | `docs/api.md`: every endpoint, query parameter and JSON shape (below), with one real example response each, produced by running the code on the fixtures. Shapes are the serde structs in core (`RunSummary`, `Comparison`, ...). |
| Server skeleton | `crates/server` with axum, `benchlens serve [--listen 127.0.0.1:8080] [--db]`, router with every endpoint returning 501, static-file handler serving `web/` (rust-embed), CI building it. |

Done when: `cargo build --target wasm32-unknown-unknown -p benchlens-rules` works, `docs/api.md` merged,
`benchlens serve` starts and serves `web/index.html` (a placeholder).

### API v1 (to be written out in docs/api.md by T0)

| Method, path | Purpose |
|---|---|
| `GET /api/runs?tag=k:v&arch=&group=&q=` | list runs (`RunSummary[]`) |
| `GET /api/runs/{id}` | one run: summary + hardware + failed stages + files list |
| `GET /api/runs/{id}/files/{name}` | a kept file (report.md, meta.json ...), `text/plain` |
| `PATCH /api/runs/{id}/tags` | `{"key": "value", "old": ""}`; empty value removes (write) |
| `DELETE /api/runs/{id}` | remove a run (write) |
| `GET /api/groups` | groups with run count, archs, vCPU, CPU |
| `GET /api/compare?base=REF&other=REF&other=REF&section=` | `Comparison` (REF = run id or `group:NAME`) |
| `GET /api/values?ref=REF&id=METRIC` | raw values of one metric for the dot plot |
| `GET /api/metrics?version=` | the metric table |
| `POST /api/runs` | upload `.tar.gz` (multipart `file`, repeated `tag=k=v`) → `ImportReport[]` (write, C2) |
| `GET /api/health` | version, DB layout version, run count |

Errors: JSON `{"error": "...", "detail": "..."}` with 400 / 404 / 401 / 413 / 500.

## Wave 1 (parallel): done, integrated by L in `integration/wave1`

| ID | Task | Owns | Builds on | Done when |
|---|---|---|---|---|
| **A** (done) | Server: implement every read endpoint and tag/delete over `core::store` / `core::compare`; request limits; structured errors; integration tests with fixtures (axum `oneshot`). | `crates/server/src/**` except `upload.rs`, `auth.rs` | T0 | every API v1 read endpoint answers as `docs/api.md` says; tests cover 404 and bad refs |
| **B** (done) | Web UI: port the prototype (Claude Artifact page) to `web/`: baseline + N picker, quick picks, tally table, heatmap, metric dot plot, runs list with tag editing, hardware table, shareable URL (state in the query string). Plain HTML + JS + d3, no bundler; vendor d3 into `web/vendor/`. Develop against JSON saved from the CLI / API on the fixtures. Light and dark theme, works at 400 px wide. | `web/**` | T0 (api.md) | all views work against `benchlens serve` on the POC data; numbers equal `benchlens compare` |
| **E** (done) | WASM: `crates/wasm` exposing `compare(values_json, base, others) -> Comparison` from `benchlens-rules`; build script producing `web/wasm/`; a node test running the golden pairs through the WASM build. | `crates/wasm/**`, `web/wasm/**` (generated) | T0 | node test passes the golden file through WASM |
| **F** (done) | Packaging: release workflow on tag `v*` (linux x86_64 + aarch64 musl, macOS arm64 + x86_64, Windows x86_64), checksums, `Dockerfile` (static binary on distroless or scratch, non-root, `/data` volume), `docker-compose.yml`. | `.github/workflows/release.yml`, `Dockerfile`, `docker-compose.yml` | M1 | a test tag builds every target; `docker run -v data:/data -p 8080:8080` serves |
| **C1** (done) | Inbox: `core::inbox` scanning a folder every N seconds (polling only: no `notify` events, since the stability rule needs a second look after a delay anyway and polling works on network shares and in containers), importing folders and archives that are complete (unchanged size for 2 scans), moving them to `done/` or `failed/` with a `.reason.txt`; `benchlens watch` and a background task in `serve`. | `crates/core/src/inbox.rs`, its tests, the `watch` subcommand | M1 | dropping a tar or folder into `inbox/` imports it once; a broken one lands in `failed/` with the reason |
| **V** (done) | vmbench (other repo): `hw.py` reports `cpu.model` "-" on HiSilicon; read the model from `/proc/cpuinfo` / `lscpu` "Model name" / MIDR mapping. | `chaloemph/vmbench` | none | ARM64 hardware.json carries a model name; benchlens fallback still works for old runs |

## Follow-ups from the Wave 1 review

| ID | Follow-up | Where | From |
|---|---|---|---|
| F1 | Group-subset reference such as `group:NAME@tag=value` (pooled runs of a group with one tag value), so "round 1 vs 2" compares pooled rounds; then switch B's quick picks to it. Contract change: api.md, core `resolve`, UI. | core, server, web | B |
| F2 | Distinguish ambiguous references in core (`Error::Ambiguous`, 400 or 409 instead of 404 with a message). | core, server, api.md | A |
| F3 | `benchlens watch`: stop cleanly on Ctrl-C between scans (the `stop` flag exists). | cli | C1 |
| F4 | Release workflow dry run (`workflow_dispatch` on GitHub) before tagging v0.1.0: five targets, checksums, multi-arch image. | release.yml | F |
| F5 | The real part number of the POC's HiSilicon CPU: rerun the `info` stage of vmbench (with `hw/cpu-id.txt`) on one ARM VM, add the part to `ARM_PARTS` if it is certain. | vmbench | V |
| F6 | Inbox imports with tags: `--registry` for `watch` / `serve --inbox` (a re-imported run now loses its registry group). | core inbox, cli | L (E2E) |
| F7 | `v_values` excludes stages with `LIKE` on a section read from results.tsv; use `instr()` (needs a DB layout bump, the view is `IF NOT EXISTS`). | core store | L |
| F8 | Web UI items for B: keep tag-form drafts across re-renders; link report.md only when kept; delete dialog says tags are not restored. | web | B (review) |
| F9 | Remove `web/dev/server` once C2 gives the real server tokens (it duplicates the API). | web/dev | L |
| F10 | `/api/values`: load one metric's values in SQL instead of the whole side. | core, server | A (review) |
| F11 | One watcher per inbox: document it or take a lock file. | core inbox | C1 |

## Wave 2

| ID | Task | Owns | Builds on | Done when |
|---|---|---|---|---|
| **C2** | Upload and auth: `POST /api/runs` (multipart, size limit, same import path as the CLI), token auth for every write endpoint (`BENCHLENS_TOKEN` or `benchlens.toml`), read access open or token-protected by config; upload form in the web UI (coordinate with B). Config file `benchlens.toml` (db, listen, inbox, token, max upload, read_public). | `crates/server/src/upload.rs`, `auth.rs`, `config.rs`; upload form in `web/` with B | A | curl upload works with a token and is refused without; oversized and unsafe archives return 413 / 400 |
| **D** | Export: `benchlens export html --base REF --other REF... -o report.html` writing one self-contained HTML (web assets + WASM + the needed values inlined) that still lets the reader change baseline; `export csv` / `export json` of values and stats; an "Export" button in the UI. | `crates/cli/src/export.rs`, export template under `web/export/` | B, E | the HTML opens from disk with no server and matches `benchlens compare` |
| **AB** | Integration pass: B against the real A, fix contract drift, end-to-end test (start server on fixtures, fetch every page's data). | small fixes in both | A, B | E2E test in CI |

**M2** = T0 + A + B + AB. **M3** = C1 + C2. **M4** = E + D. Release `v0.1.0` after M4 with F.

## L: lead review, refactor and integration (after every wave)

One lead agent, started when every task of a wave has reported done. Nothing reaches a PR without it.

| Step | What L does |
|---|---|
| 1. Review each branch | Read the whole diff against its task row and docs/api.md / docs/contract.md: correctness, untrusted-input handling (archives, uploads, tags, query strings, HTML), error messages, tests that really test the claim, no invented data, no change to the verdict rule. Write findings per branch in `docs/reviews/<wave>-<task>.md`. |
| 2. Fix or send back | Small issues: fix on the task branch in a separate commit (`review: ...`). Large ones: write them up and hand back to the task's agent; do not rewrite the task. |
| 3. Refactor across branches | Remove duplication between tasks (shared helpers, error mapping, config handling, test setup), align naming and module layout, keep public APIs and docs/api.md consistent. Behaviour-preserving only; separate commits. |
| 4. Integrate | Merge the wave's branches in dependency order into `integration/<wave>` (e.g. T0 → A → C1 → E → F → B), resolving conflicts (Cargo.toml / Cargo.lock / main.rs / server lib.rs are the usual ones). After each merge: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all`, the WASM golden test, and an end-to-end check (`benchlens serve` on the POC fixtures, every API endpoint and every UI view loads). |
| 5. Open PRs | Push and open one PR per task (or one integration PR if the owner prefers), each with: what changed, L's review summary and fixes, tests run and results, known gaps. CI must be green. L never merges. |
| 6. Update the plan | Mark tasks done, note follow-ups found in review as new rows, adjust the next wave's task prompts if the code moved. |

L owns: `integration/*` branches, `docs/reviews/**`, review/refactor commits on task branches, PR descriptions.
L does not: merge to `main`, change the verdict rule, add features beyond what a task row asked for.

Gate: a PR without an L review summary is not merged.

## Wave 3 (after M4)

| ID | Task | Done when |
|---|---|---|
| **G** | Grafana: `deploy/grafana/` provisioning for the SQLite datasource (frser-sqlite-datasource) on the benchlens DB (read-only mount), dashboards: values over time per metric and group, group vs group bars; compose profile `grafana`. Document that verdicts live in benchlens, not Grafana. | `docker compose --profile grafana up` shows both |
| **H** | Ecosystem docs: user guide (import routes, tags, groups, compare, export), CONTRIBUTING (adding a vmbench version: catalog + fixture + golden), security notes for running on a server, example data walkthrough with `examples/poc-2026-11`. | someone outside the team gets from clone to a comparison with the README alone |

## Decisions (2026-11-19)

1. Repo: `chaloemph/benchlens` on GitHub, public; agents work through PRs and CI.
2. Merging: the project owner merges every PR. Task agents only commit locally; the lead agent (task L) reviews,
   refactors, integrates and opens the PRs, and every PR must have passed L.
3. Web UI (B): follows the Claude Artifact prototype (baseline + N picker, quick picks, tally, heatmap,
   dot plot), plus the runs list and hardware views.
4. T0: done by the lead session before Wave 1 is handed out.
