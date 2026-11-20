# Wave 3 review: G (Grafana profile, read-only SQLite datasource, three dashboards)

Branch `m3/g-grafana` at f140e0d, reviewed against docs/plan.md (Wave 3, G). Verdict: **ok, fixed in review**.

## What was checked

- **No secrets committed**: the datasource has only a path and `mode=ro`; the admin password comes from
  `GRAFANA_ADMIN_PASSWORD`; analytics and update checks are off.
- **Read-only**: the benchlens volume is mounted `:ro` and opened with `mode=ro`. Checked on the running
  stack: `DELETE FROM tags` through `/api/ds/query` answers "attempt to write a readonly database"; the
  plugin sets the attached-database limit to 0, so `ATTACH '/var/lib/grafana/grafana.db'` is refused
  ("too many attached databases - max 0"). An anonymous Viewer can send any read-only SQL to the benchlens
  database through `/api/ds/query`: Grafana exposes everything in the file, also when benchlens runs with
  `read_public = false`. Documented in SECURITY.md.
- **Dashboards**: Metric over time, Group vs group (pooled `SUM(mean*n)/SUM(n)`, min, max, n, labelled as
  means) and Runs; each says verdicts live in benchlens and links to its UI. Template variables use
  `:sqlstring` (except `metric`, fixed below). Versions pinned (Grafana 13.3.1, plugin 4.0.6).
- **Verdicts**: none computed in SQL; the "means are not verdicts" note is on every dashboard and in the
  README. Matches the plan.

## Decisions and fixes

- **SQLITE_FCNTL_PERSIST_WAL (G's suggestion): added.** Safe and small: `Store::init` sets the file control
  on every connection; the last connection still checkpoints the WAL into `benchlens.db`, but keeps
  `-wal` and `-shm` instead of deleting them. That is the documented way to let read-only readers open a
  WAL database (sqlite.org/wal.html, "Read-Only Databases"). Costs: two files that stay next to the
  database (already in .gitignore), and a `-wal` that keeps its largest size until the next checkpoint
  reset. Test `read_only_reader_works_after_benchlens_closed_the_database` (fails without the setting).
  Checked with compose: `docker compose stop benchlens`, then every dashboard query still answers; a
  `docker compose run benchlens tag ...` while serve is stopped is visible in Grafana on the next query.
  deploy/grafana/README.md rewritten for it.
- **Admin password**: `${GRAFANA_ADMIN_PASSWORD:?}` was considered and rejected: compose interpolates every
  service, including those of inactive profiles, so `docker compose up` without the profile would fail too
  (checked: "required variable GRAFANA_ADMIN_PASSWORD is missing a value"). Instead the port is published
  on `${GRAFANA_BIND:-127.0.0.1}` so the `admin` / anonymous-Viewer defaults are local only, and the
  README, deploy/grafana/README.md, the guide and SECURITY.md carry a server checklist (password before the
  first start, `GRAFANA_ANONYMOUS=false`, `GRAFANA_BIND=0.0.0.0` behind TLS).
- `review(G)`: `'${metric}'` was interpolated raw inside quotes; now `${metric:sqlstring}` like the rest.

## Grafana check with the integrated image

`docker compose --profile grafana up -d` with the image built from integration/wave3, fixtures imported
with the POC registry, then every panel query of the three dashboards through `/api/ds/query` with all
variable values: no errors (Metric over time 5 + 5 rows for the first metric, Group vs group 213 + 3 rows,
Runs 1 + 7 rows), the same with benchlens stopped. `docker compose down -v` afterwards.

## For later (not blocking)

- The Runs dashboard's "values" column counts raw `results` rows (failed and excluded stages included),
  like `RunSummary.values`; fine, but worth a column note.
- The `benchlens UI` text variable is a free URL used in data links; a crafted dashboard link can point
  those links elsewhere. Grafana sanitises `javascript:` URLs; low risk.
