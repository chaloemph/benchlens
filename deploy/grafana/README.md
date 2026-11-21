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

Open http://localhost:3000 and log in as `admin` / `$GRAFANA_ADMIN_PASSWORD`. Anonymous access is **off**
by default: without a login Grafana answers only its login page.

> **Change the admin password.** Without `GRAFANA_ADMIN_PASSWORD` the password is `admin`. Set it before the
> first start: Grafana keeps the password it was first started with in the `grafana-data` volume, so setting
> the variable later changes nothing (change it in the UI, or with `docker compose exec grafana grafana cli
> admin reset-admin-password ...`).

**Anonymous viewers, for a local machine only.** To look at the dashboards without logging in, opt in:

```bash
GRAFANA_ANONYMOUS=true GRAFANA_ADMIN_PASSWORD=change-me docker compose --profile grafana up -d
```

Anonymous visitors are then Viewers; `admin` can still log in and edit.

**Grafana bypasses benchlens' access rules.** Grafana reads the database file itself, not the benchlens API,
so benchlens' token and `read_public = false` do not apply to it. Every Grafana user, and with
`GRAFANA_ANONYMOUS=true` everyone who reaches port 3000, can see every run, and a Viewer can send any
read-only SQL to the database through Grafana's query API (`/api/ds/query`), not only the dashboards'
queries. For a private benchlens (`read_public = false`), do not expose Grafana to anyone who should not see
all the data: keep it on 127.0.0.1, keep anonymous access off, and give Grafana logins only to people who
may read everything.

**Defaults are for a local machine.** Compose publishes the port on **127.0.0.1 only**. On a server:

- set `GRAFANA_ADMIN_PASSWORD` to a real password before the first start (above);
- leave `GRAFANA_ANONYMOUS` unset (off) unless the results are meant for everyone who can reach the port;
- set `GRAFANA_BIND=0.0.0.0` (or a LAN address) to publish the port, preferably behind a reverse proxy with
  TLS, like benchlens itself.

The compose file does not refuse to start without `GRAFANA_ADMIN_PASSWORD` on purpose: compose checks
variables of every service, so a required variable would also break `docker compose up` without the profile.

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
in the box or in the URL, `?var-benchlens_url=https://bench.example.org`; benchlens must serve that name: the
container answers only `localhost` and IP addresses unless `BENCHLENS_ALLOWED_HOSTS` or `allowed_hosts` in a
mounted `benchlens.toml` lists it).

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
opened read-only. That works **as long as `-wal` and `-shm` exist**: they do while `benchlens serve` has the
database open (in compose, the `benchlens` service, `restart: unless-stopped`, and Grafana `depends_on` it),
and benchlens keeps them when it closes the database (below).

Tested with this setup (Grafana 13.3.1, plugin 4.0.6, 7 fixture runs): 320 Grafana queries
(`/api/ds/query`) while runs were deleted and imported 13 times by `docker compose run benchlens ...`
(separate writer processes, including WAL checkpoints): no errors, every result a consistent snapshot
(before or after a write, never in between), and new data visible on the next query without restarting Grafana.

**When benchlens is stopped**, the files stay: benchlens sets `SQLITE_FCNTL_PERSIST_WAL` on its connections,
so its last connection checkpoints the WAL into `benchlens.db` but keeps `-wal` and `-shm` instead of deleting
them, and Grafana keeps reading the database (the last state benchlens wrote). Without that, a read-only
reader cannot create the two files and panels show `unable to open database file (14)`. That is still what
happens with a database file that was copied without its `-wal` and `-shm`, or last closed by a benchlens
build older than v0.1.0: start benchlens once (`docker compose up -d benchlens`) to create them.

For a frozen database that nothing writes to (an archived copy), the datasource's `pathOptions` can also be
`mode=ro&immutable=1`. Never use `immutable=1` on a database benchlens is writing: SQLite then skips all
locking and change detection and can return wrong results.

(A read-write mount alone would not help: the volume belongs to the benchlens user, uid 65532, and Grafana runs
as uid 472, so it could not create `-shm` there either.)

Not chosen: a periodic snapshot copy for Grafana. It needs its own job and makes the dashboards lag, while
reading the live file works.
