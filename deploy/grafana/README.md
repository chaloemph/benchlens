# Grafana on the benchlens database

Grafana with the SQLite datasource ([frser-sqlite-datasource](https://grafana.com/grafana/plugins/frser-sqlite-datasource/))
reading the benchlens database **read-only**, and three provisioned dashboards. Grafana draws values, means and
ranges; **verdicts (better / worse / within noise) live in benchlens only** and every dashboard says so and
links to the benchlens UI.

```bash
# from a checkout: benchlens on :8080 and Grafana on :3000
BENCHLENS_TOKEN=$(openssl rand -hex 32) GRAFANA_ADMIN_PASSWORD=change-me \
  docker compose --profile grafana up -d
# import results through the benchlens container (or upload them in the web UI with the token)
docker compose run --rm -v "$PWD/results:/results:ro" \
  -v "$PWD/examples/poc-2026-11/registry.tsv:/registry.tsv:ro" benchlens import --registry /registry.tsv /results
```

Open http://localhost:3000: anonymous visitors are Viewers (set `GRAFANA_ANONYMOUS=false` to require a login);
`admin` / `$GRAFANA_ADMIN_PASSWORD` (default `admin`, set your own) can edit. Port 3000 has no TLS: on a
server, put it behind a reverse proxy like benchlens itself.

## Versions

| What | Version | Where |
|---|---|---|
| Grafana | `grafana/grafana:13.3.1` | `docker-compose.yml` |
| SQLite datasource | `frser-sqlite-datasource@4.0.6` | `GF_PLUGINS_PREINSTALL_SYNC` in `docker-compose.yml` |

The plugin is downloaded from grafana.com on first start (needs network) and kept in the `grafana-data`
volume. To upgrade, change both pins together and check the dashboards.

## Files

```
deploy/grafana/
  provisioning/datasources/benchlens.yaml   datasource "benchlens" (uid benchlens), path, mode=ro
  provisioning/dashboards/benchlens.yaml    loads dashboards/ into the folder "benchlens", not editable in the UI
  provisioning/{plugins,alerting}/          empty, so Grafana does not log missing folders
  dashboards/metric-over-time.json          home dashboard
  dashboards/group-vs-group.json
  dashboards/runs.json
```

Mounted by compose: the benchlens volume at `/var/lib/benchlens` (`:ro`), `provisioning/` at
`/etc/grafana/provisioning`, `dashboards/` at `/etc/grafana/dashboards/benchlens`.

## Dashboards

Template variables: **Section** (`cpu`, `disk`, `pg`, ...), **Group** (the effective group: `group` tag, else the
automatic one), **Metric** (narrowed by section and group), **Run** (narrowed by group) and **benchlens UI**
(a text box, default `http://localhost:8080`: where *your browser* reaches benchlens; used by the links. Set it
in the box or in the URL, `?var-benchlens_url=https://bench.example.org`).

| Dashboard | Panels | Source |
|---|---|---|
| **Metric over time** | one metric, one series per group, a point per run at its `start_utc` (time range filters on it); table of those runs with n, mean, min, max | `v_stats` joined with `v_runs` |
| **Group vs group** | per metric and group: mean of all counted rounds of the group's runs, min, max, n, runs; a bar chart (min / mean / max per group) repeated for every selected metric | `v_stats`, pooled: `SUM(mean*n)/SUM(n)`, `MIN(min)`, `MAX(max)`, `SUM(n)` |
| **Runs** | counts; every run with group, vmbench version, failed stages, tags, arch, vCPU, RAM, CPU, stages, catalog known, number of values, run id | `v_runs`, `tags`, `failed_stages` |

Values of failed stages and of stages in a run's `exclude` tag are left out (that is how `v_values` and
`v_stats` are defined). Run and group cells link to the benchlens hardware and compare views.

**What the numbers are not.** SQLite has no median, so Grafana shows **means** (labelled as such). benchlens
compares **medians** and decides with vmbench's noise rule (CV); a gap between two means in Grafana is not a
verdict and can disagree with benchlens. For "is B better than A", use `benchlens compare` or the benchlens UI.

## Read-only database and SQLite WAL

benchlens keeps its database in WAL mode (`benchlens.db` plus `benchlens.db-wal` and `benchlens.db-shm`). Grafana
gets the benchlens volume mounted **`:ro`** and opens the file with `mode=ro`, so it cannot change anything
(a write query answers `attempt to write a readonly database`).

A WAL reader normally writes to the `-shm` file (shared-memory index). On a read-only mount it cannot, and
SQLite (3.22 and later) then falls back to reading the WAL itself on every transaction, with the `-shm` file
opened read-only. That works **as long as `-wal` and `-shm` exist**, which they do while `benchlens serve`
has the database open: in compose, the `benchlens` service (`restart: unless-stopped`, and Grafana
`depends_on` it).

Tested with this setup (Grafana 13.3.1, plugin 4.0.6, 7 fixture runs): 320 Grafana queries
(`/api/ds/query`) while runs were deleted and imported 13 times by `docker compose run benchlens ...`
(separate writer processes, including WAL checkpoints): no errors, every result a consistent snapshot
(before or after a write, never in between), and new data visible on the next query without restarting Grafana.

**When benchlens is stopped**, its last connection checkpoints and deletes `-wal` and `-shm`; a read-only
reader cannot create them, and panels show `unable to open database file (14)` until benchlens runs again
(no Grafana restart needed). Options if Grafana must work without a running benchlens:

- keep `benchlens serve` running (the compose default);
- a frozen database that nothing writes to (an archived copy): set the datasource's `pathOptions` to
  `mode=ro&immutable=1`. Never use `immutable=1` on a database benchlens is writing: SQLite then skips all
  locking and change detection and can return wrong results.

(A read-write mount alone would not help: the volume belongs to the benchlens user, uid 65532, and Grafana runs
as uid 472, so it could not create `-shm` there either.)

Not chosen: a periodic snapshot copy for Grafana. It needs its own job and makes the dashboards lag, while
reading the live file works.
