# benchlens plan: M2 onwards, split for parallel agents

Status 2026-11-20: **M1 done** (commit 298860c): core library, CLI, SQLite store, golden tests against vmbench.
**T0 done** (PR #1). **Wave 1 done** (A, B, C1, E, F reviewed and integrated in `integration/wave1`; V in a
vmbench PR); reviews in `docs/reviews/wave1-*.md`. **Wave 2 done** (K, I, C2, D, W, AB reviewed and integrated
in `integration/wave2`); reviews in `docs/reviews/wave2-*.md`. With it **M2, M3 and M4 are complete**; the F4
release dry run passed. **Wave 3 done** (S, U, G, H reviewed and integrated in `integration/wave3`; F5 and
F12-F18 done); reviews in `docs/reviews/wave3-*.md`. **Ready for v0.1.0**: see
[Releasing v0.1.0](#releasing-v010). Follow-ups below.
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
| F1 (done, K) | Group-subset reference such as `group:NAME@tag=value` (pooled runs of a group with one tag value), so "round 1 vs 2" compares pooled rounds; then switch B's quick picks to it. Contract change: api.md, core `resolve`, UI. | core, server, web | B |
| F2 (done, K) | Distinguish ambiguous references in core (`Error::Ambiguous`, 400 or 409 instead of 404 with a message). | core, server, api.md | A |
| F3 (done, I) | `benchlens watch`: stop cleanly on Ctrl-C between scans (the `stop` flag exists). | cli | C1 |
| F4 (dry run done, L) | Release workflow dry run (`workflow_dispatch` on GitHub) before tagging v0.1.0: five targets, checksums, multi-arch image. | release.yml | F Dry run 2026-11-20 on `integration/wave2` (run 44670841928): all five targets built and packaged, SHA256SUMS made, multi-arch image built (not pushed); the first run failed in the docker job (registry cache race, fixed in the Dockerfile). Left: tag `v0.1.0` after the merge. |
| F5 (done, main session) | The real part number of the POC's HiSilicon CPU: rerun the `info` stage of vmbench (with `hw/cpu-id.txt`) on one ARM VM, add the part to `ARM_PARTS` if it is certain. Done 2026-11-20 on the arm64 POC VMs: implementer 0x48 (HiSilicon), part 0xd03, r0p0, `midr_el1` 0x00000000480fd030, SVE2. vmbench v0.2.4 reports it as "HiSilicon part 0xd03", which is correct; the marketing name of 0xd03 is not confirmed, so nothing is added to the table. | vmbench | V |
| F6 (done, I) | Inbox imports with tags: `--registry` for `watch` / `serve --inbox` (a re-imported run now loses its registry group). | core inbox, cli | L (E2E) |
| F7 (done, K) | `v_values` excludes stages with `LIKE` on a section read from results.tsv; use `instr()` (needs a DB layout bump, the view is `IF NOT EXISTS`). | core store | L |
| F8 (done, W) | Web UI items for B: keep tag-form drafts across re-renders; link report.md only when kept; delete dialog says tags are not restored. | web | B (review) |
| F9 (done, C2) | Remove `web/dev/server` once C2 gives the real server tokens (it duplicates the API). | web/dev | L |
| F10 (done, K) | `/api/values`: load one metric's values in SQL instead of the whole side. | core, server | A (review) |
| F11 (done, I) | One watcher per inbox: document it or take a lock file. | core inbox | C1 |

## Follow-ups from the Wave 2 review

| ID | Follow-up | Where | From |
|---|---|---|---|
| F12 (done, U) | `has_report` (or the `files` list) in `RunSummary`, so the Runs view needs no RunDetail request per row for the report.md link. API change: api.md, core, UI. Done: `kept_files` (sorted names; not `files`, which RunDetail already has with sizes). | core, server, web | W |
| F13 (done, U) | On a private server (`read_public = false`) the report.md link opens in a new tab without the token and gets 401: fetch it with the token into a blob, or offer a short-lived signed link. Done: a click fetches it with the token and shows it as plain text in a dialog (a new tab still gets 401, fails closed). | web | L (C2 review) |
| F14 (done, S) | DNS rebinding against a loopback server without a token: any `Host` is accepted, so a rebinding page is same-origin and can read and write. Accept only localhost / 127.0.0.1 / [::1] as `Host` in no-token mode, or always require a token. Done: without a token only `localhost`, IP addresses and `allowed_hosts` are served (403 otherwise); with a token, `allowed_hosts` when set. Every name a request gives must pass (L review). | server auth | L (C2 review) |
| F15 (done, S) | Reverse proxy on the same host: loopback without a token means "writes open". Documented in the README; consider refusing open writes when `Forwarded` / `X-Forwarded-For` is present. Done: open writes carrying `Forwarded` / `X-Forwarded-For` / `X-Forwarded-Host` are 403. | server auth | L (C2 review) |
| F16 (done, S) | Uploads get no registry: an uploaded run without a `group` tag lands in its automatic group. Apply the server's `inbox_registry` to uploads too (and say so in the dialog). Done: setting renamed `registry` (`inbox_registry` kept as an alias), read at every upload, form tags win. | server upload | L (E2E) |
| F17 (done, U) | The compare picker has no control for `group:NAME@KEY=VALUE` sides (only quick picks and links make them); a "split this group by tag" control. Done: "Compare with: a group split by tag" (group, tag, values; optionally the first as baseline). | web | L (K review) |
| F18 (done, S) | `benchlens compare ... \| head` panics with "failed printing to stdout: Broken pipe"; exit quietly on EPIPE. Done: every command but `serve` restores the default SIGPIPE action and ends quietly by SIGPIPE (shell status 141). | cli | L (E2E) |

## Follow-ups from the Wave 3 review

| ID | Follow-up | Where | From |
|---|---|---|---|
| F19 (done, M4) | With a token and `read_public = true`, any `Host` is served, so a DNS rebinding page can read (not write) the public data of a server on 127.0.0.1. Documented (SECURITY.md); consider applying the Host check by default when listening on loopback, also with a token (mind proxies that pass the public name). Done: a loopback listener checks the `Host` also with a token (`localhost`, IP addresses, `allowed_hosts`); the 403 names the exact `allowed_hosts` / `BENCHLENS_ALLOWED_HOSTS` setting for the refused name (for a proxy on the same host that passes its public name); non-loopback listeners unchanged. Reviewed by L (docs/reviews/m4-followups.md): Docker (0.0.0.0) is not covered, documented with `BENCHLENS_ALLOWED_HOSTS=localhost` (F23). | server auth | L (S review) |
| F20 (done, M4) | Grafana's anonymous Viewer can send any read-only SQL to the benchlens database (`/api/ds/query`), so Grafana shows everything even when benchlens has `read_public = false`. Documented; consider `GRAFANA_ANONYMOUS` defaulting to false when benchlens is private, or a note in the compose file. Done: `GRAFANA_ANONYMOUS` defaults to false (login needed; `GRAFANA_ANONYMOUS=true` to opt in on a local machine); README, guide, SECURITY.md and deploy/grafana/README.md say to change the `admin` password and that Grafana bypasses `read_public`. Checked with `docker compose --profile grafana up -d`: anonymous 401 / redirect to login, admin login works. | deploy/grafana | L (G review) |
| F21 (done, M4) | The release notes come from `gh release create --generate-notes`; consider using the CHANGELOG section of the tag instead. Done: `scripts/release-notes.sh VERSION` extracts the `## [X.Y.Z]` section (fails when missing, empty or "unreleased"); release.yml runs it in a first job `notes` (build, docker and release wait for it), prints the notes (also on the dry run) and uses them with `--notes-file`. | release.yml | L |
| F22 (done, M4) | No MSRV job in CI: `rust-version = 1.89` (workspace; rules 1.80, wasm 1.81 on purpose) is only checked by hand (`cargo +1.89 check --locked --all-targets` passed on `integration/wave3`). Add a CI job with the 1.89 toolchain. Done: CI job `msrv` reads `rust-version` from `[workspace.package]` and runs `cargo check --locked --workspace --all-targets` with that toolchain. | ci.yml | L |
| F23 (done, M5) | Docker: the container listens on 0.0.0.0, so with a token it serves any `Host` (F19 covers loopback listeners only) and a rebinding page can read public data through a published port, also `127.0.0.1:8080:8080`. Documented with the workaround `BENCHLENS_ALLOWED_HOSTS=localhost` (SECURITY.md, CHANGELOG, compose comment); consider setting it in docker-compose.yml by default, or a `--host-check` switch. Done: the Dockerfile sets `ENV BENCHLENS_ALLOWED_HOSTS=localhost`, so a container checks the `Host` also with a token (`localhost`, IP addresses). An explicit `BENCHLENS_ALLOWED_HOSTS` (`docker run -e`, compose) replaces the default instead of adding to it: plain Docker semantics, and nothing is lost since `localhost` and IP addresses are always served once the check is on; an empty value turns it off (documented, not recommended). Compose has a commented `BENCHLENS_ALLOWED_HOSTS: localhost,bench.example.org`. Behaviour change for containers opened by a name (CHANGELOG, README, guide, SECURITY.md). Checked with docker: `Host: evil.example` 403, `localhost` / IP 200, `-e BENCHLENS_ALLOWED_HOSTS=bench.lan` serves bench.lan and localhost; no token still refuses writes (403). Reviewed by L (docs/reviews/m5-f23-f24.md): the env default silently hid `allowed_hosts` of a mounted benchlens.toml; `serve` now warns at start-up when that happens; docs name proxy containers that reach benchlens by service name (F25, F26). | docker-compose.yml, server auth | L (M4 review) |
| F24 (done, M5) | A listen address in IPv4-mapped form (`[::ffff:127.0.0.1]`) is not treated as loopback (`Ipv6Addr::is_loopback` is false for it): with a token any `Host` is served, without one writes are closed. Fail-safe for writes, and an unusual way to listen; consider `ip.to_canonical().is_loopback()` in `Access::for_listen`, `ServeConfig` validation and the start-up message together. Done: `config::is_loopback` (`ip.to_canonical().is_loopback()`) is used by `Access::for_listen`, the no-token warning and the start-up line; `::ffff:127.x.y.z` is loopback for the Host check and for open writes, `::ffff:192.168.1.5` stays non-loopback. `Host: [::ffff:127.0.0.1]` was already accepted as an IP. Unit and router tests; checked live with `--listen [::ffff:127.0.0.1]:PORT`. | server auth | L (M4 review) |

## Follow-ups from the M5 review (F23, F24)

| ID | Follow-up | Where | From |
|---|---|---|---|
| F25 (done, M6) | The Host 403 always suggests `allowed_hosts = [...]` in benchlens.toml or `BENCHLENS_ALLOWED_HOSTS`, but when the variable is set (always in the Docker image) the file's list has no effect; the start-up warning covers it. Consider passing the source of the list to `Access` and naming only the setting that works (without showing the configured names). Done: `Settings` records `allowed_hosts_source` (`HostsSource`: flag, env, file, image default, unset), `Access::for_settings` passes it on; the 403 names `BENCHLENS_ALLOWED_HOSTS=name` for a flag/env list, `allowed_hosts = ["name"]` for a file list, both for the image default or no list; configured names and the token never shown. The start-up line says `(from <source>)`. Unit tests per source (`auth::tests::host_refused_names_the_setting_of_the_source`), router test through the real middleware with resolved settings (`hardening::image_default_and_mounted_file_through_the_router`). Reviewed by L (docs/reviews/m6-f25-f26.md): a flag source named the variable (fixed: names the command-line list); without a token the 403 suggested "or set a token" also when a list was set, where a token does not help (fixed: only with no list). | server auth | L (M5 review) |
| F26 (done, M6) | The image default `BENCHLENS_ALLOWED_HOSTS=localhost` sits in the environment, so it beats `allowed_hosts` of a mounted benchlens.toml; users of a config file must add `-e BENCHLENS_ALLOWED_HOSTS=` or repeat the names. Kept for plain env semantics and documented (warning at start-up). Revisit if users trip over it: e.g. a lower-priority built-in default for containers instead of an `ENV`. Done: the image sets `BENCHLENS_DEFAULT_ALLOWED_HOSTS=localhost`, the lowest-priority source (flag > `BENCHLENS_ALLOWED_HOSTS` > file > default > none), so a mounted benchlens.toml replaces it; an empty list counts as not set; `BENCHLENS_ALLOWED_HOSTS=` (set, empty) also drops the default, keeping the 0.1.2 opt-out. The start-up warning only fires when a flag or `BENCHLENS_ALLOWED_HOSTS` hides a different non-empty file list. Behaviour change only for mounted-file users (CHANGELOG). Docker-checked: file `bench.lan` 200 / evil 403 with no extra env; no file localhost 200 / evil 403; `-e BENCHLENS_ALLOWED_HOSTS=other.lan` (with and without file); `-e BENCHLENS_ALLOWED_HOSTS=` any Host / file list. Reviewed by L (docs/reviews/m6-f25-f26.md): `BENCHLENS_ALLOWED_HOSTS=" "` (no names) turned the check off and hid the file's list, unlike `=`; it now counts as empty too (CHANGELOG). | Dockerfile, server config | L (M5 review) |

## Follow-ups from the M6 review (F25, F26)

| ID | Follow-up | Where | From |
|---|---|---|---|
| F27 (done, M7) | No start-up warning when the Host check is off on a non-loopback listener with a token (`Host names served: any` is the only hint), e.g. a container started with `-e BENCHLENS_ALLOWED_HOSTS=` and no `allowed_hosts` in a file. Consider a warning like the no-token one, naming the opt-out and `allowed_hosts`. Done: `Settings::warnings()` adds `warning: listening on <addr> with no Host check: ...` when a token, a non-loopback listener, `read_public = true` and an empty `allowed_hosts` meet (`Access::checks_host` itself, plus open reads; L review: the warning now asks `checks_host` instead of repeating its condition); it names `allowed_hosts` / `BENCHLENS_ALLOWED_HOSTS=name` and `read_public = false`, never the token or configured names. Quiet with a list from any source (image default included), on loopback, with `read_public = false` (only `/api/health` and the UI files are open; L review: accepted, documented in SECURITY.md), and without a token (the old warning). Unit tests (`config::tests::warns_when_the_host_check_is_off`), router test of the opt-out (`hardening`), binary stderr test (`crates/cli/tests/serve_warnings.rs`); Docker-checked (opt-out warns in `docker logs`, image default quiet). | server config | L (M6 review) |

## Follow-ups from the M7 review (F27)

| ID | Follow-up | Where | From |
|---|---|---|---|
| F28 | Start-up warnings print the configured listen address, so `serve --listen 0.0.0.0:0` reads `0.0.0.0:0` while the first line shows the bound port. Use the bound address (low). | server serve / config | L (M7 review) |
| F29 | `HostsSource::Flag` (and its 403 text) is reachable only from code: `benchlens serve` has no `--allowed-hosts` flag. Add the flag (docs/guide.md table) or drop the source (low). | cli / server config | L (M7 review) |

## Wave 2: done, integrated by L in `integration/wave2`

K (F1, F2, F7, F10), I (F3, F6, F11) and W (F8) took the Wave 1 follow-ups; C2, D and AB are the rows below.

| ID | Task | Owns | Builds on | Done when |
|---|---|---|---|---|
| **C2** (done) | Upload and auth: `POST /api/runs` (multipart, size limit, same import path as the CLI), token auth for every write endpoint (`BENCHLENS_TOKEN` or `benchlens.toml`), read access open or token-protected by config; upload form in the web UI (coordinate with B). Config file `benchlens.toml` (db, listen, inbox, token, max upload, read_public). | `crates/server/src/upload.rs`, `auth.rs`, `config.rs`; upload form in `web/` with B | A | curl upload works with a token and is refused without; oversized and unsafe archives return 413 / 400 |
| **D** (done) | Export: `benchlens export html --base REF --other REF... -o report.html` writing one self-contained HTML (web assets + WASM + the needed values inlined) that still lets the reader change baseline; `export csv` / `export json` of values and stats; an "Export" button in the UI. | `crates/cli/src/export.rs`, export template under `web/export/` | B, E | the HTML opens from disk with no server and matches `benchlens compare` |
| **AB** (done) | Integration pass: B against the real A, fix contract drift, end-to-end test (start server on fixtures, fetch every page's data). | small fixes in both | A, B | E2E test in CI |

**M2** = T0 + A + B + AB. **M3** = C1 + C2. **M4** = E + D. Release `v0.1.0` after M4 with F. All three are
complete with Wave 2 and the F4 dry run passed; tagging `v0.1.0` is the last step.

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

## Wave 3 (after M4): done, integrated by L in `integration/wave3`

S (F14, F15, F16, F18) and U (F12, F13, F17) took the Wave 2 follow-ups; G and H are the rows below.

| ID | Task | Done when |
|---|---|---|
| **G** (done) | Grafana: `deploy/grafana/` provisioning for the SQLite datasource (frser-sqlite-datasource) on the benchlens DB (read-only mount), dashboards: values over time per metric and group, group vs group bars; compose profile `grafana`. Document that verdicts live in benchlens, not Grafana. | `docker compose --profile grafana up` shows both |
| **H** (done) | Ecosystem docs: user guide (import routes, tags, groups, compare, export), CONTRIBUTING (adding a vmbench version: catalog + fixture + golden), security notes for running on a server, example data walkthrough with `examples/poc-2026-11`. | someone outside the team gets from clone to a comparison with the README alone |

## Releasing v0.1.0

**Released 2026-11-20** (tag `v0.1.0` on the Wave 3 merge commit 51855e0). v0.1.1 (F19-F22), v0.1.2 (F23-F24), v0.1.3 (F25-F26) and v0.1.4 (F27) follow the same steps; a
release PR first bumps `[workspace.package] version` (and `Cargo.lock`), dates the CHANGELOG heading and rebuilds
`web/wasm` (the wasm embeds the version), because `release.yml` refuses a tag that does not match them. The steps below are what was done and
the template for the next release.

Everything for v0.1.0 is on `integration/wave3`: the workspace version is `0.1.0`, CHANGELOG lists Waves 1-3
under `[0.1.0] - unreleased`, and the release workflow passed a dry run on that branch (see the PR). Steps for
the owner:

1. Merge the Wave 3 PR into `main` (optionally first set the date in CHANGELOG's `[0.1.0]` heading in the PR;
   the GitHub release shows the date either way).
   For later releases: rename CHANGELOG's `[Unreleased]` section to `## [X.Y.Z] - YYYY-MM-DD` and set the
   workspace version; `scripts/release-notes.sh X.Y.Z` must print the notes (the release fails otherwise).
2. Tag the merge commit and push the tag:
   `git fetch origin && git tag -a v0.1.0 -m "benchlens v0.1.0" origin/main && git push origin v0.1.0`
   (check first that `origin/main` is the merge commit: `git log -1 origin/main`).
3. `release.yml` does the rest: it checks the tag against the workspace version, builds the five targets,
   writes `SHA256SUMS`, creates the GitHub release with the archives (since 0.1.1 with the tag's CHANGELOG
   section as notes; 0.1.0 used `--generate-notes`), and pushes the multi-arch image
   `ghcr.io/chaloemph/benchlens` (`0.1.0`, `0.1`, `latest`, `sha-...`). Watch it with
   `gh run watch $(gh run list --workflow release.yml -L1 --json databaseId -q '.[0].databaseId') --exit-status`.
4. After it: make the ghcr package public if it is not (GitHub, package settings), and check
   `docker run --rm ghcr.io/chaloemph/benchlens:0.1.0 --version`.

## Decisions (2026-11-19)

1. Repo: `chaloemph/benchlens` on GitHub, public; agents work through PRs and CI.
2. Merging: the project owner merges every PR. Task agents only commit locally; the lead agent (task L) reviews,
   refactors, integrates and opens the PRs, and every PR must have passed L.
3. Web UI (B): follows the Claude Artifact prototype (baseline + N picker, quick picks, tally, heatmap,
   dot plot), plus the runs list and hardware views.
4. T0: done by the lead session before Wave 1 is handed out.
