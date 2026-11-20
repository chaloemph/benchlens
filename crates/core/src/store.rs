//! The SQLite database: runs, their raw values, tags and the metric tables, plus views for other tools
//! (Grafana and plain SQL): `v_runs`, `v_values` and `v_stats`.

use crate::catalog::{self, Catalog};
use crate::contract::RunData;
use crate::verdict::Direction;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Version of the database layout (PRAGMA user_version).
///
/// - 1: first layout.
/// - 2: `v_values` checks the `exclude` tag as an exact comma list (layout 1 used `LIKE`, where `_` and `%`
///   in a section name were wildcards). Tables are unchanged; opening a layout 1 database recreates the views.
pub const DB_VERSION: i64 = 2;

/// The views of [`SCHEMA`], dropped before it runs when an older layout is opened (they are created with
/// `IF NOT EXISTS`, so a changed view would otherwise stay as it was). Dependents first.
const VIEWS: [&str; 3] = ["v_stats", "v_values", "v_runs"];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS runs (
  run_id          TEXT PRIMARY KEY,
  folder          TEXT NOT NULL,
  label           TEXT NOT NULL,
  arch            TEXT NOT NULL DEFAULT '',
  nproc           INTEGER,
  ram_gb          REAL,
  cpu_model       TEXT NOT NULL DEFAULT '',
  vmbench_version TEXT NOT NULL DEFAULT '',
  pg_profile      TEXT NOT NULL DEFAULT '',
  web_cpus        TEXT NOT NULL DEFAULT '',
  stages          TEXT NOT NULL DEFAULT '',
  start_utc       TEXT NOT NULL DEFAULT '',
  end_utc         TEXT NOT NULL DEFAULT '',
  auto_group      TEXT NOT NULL,
  source          TEXT NOT NULL,
  imported_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  meta_json       TEXT NOT NULL,
  hardware_json   TEXT
);
CREATE TABLE IF NOT EXISTS tags (
  run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  key    TEXT NOT NULL,
  value  TEXT NOT NULL,
  PRIMARY KEY (run_id, key)
);
CREATE TABLE IF NOT EXISTS results (
  run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  id     TEXT NOT NULL,
  metric TEXT NOT NULL,
  round  INTEGER NOT NULL,
  value  REAL NOT NULL,
  unit   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS results_run_id ON results(run_id, id);
CREATE TABLE IF NOT EXISTS failed_stages (
  run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  stage  TEXT NOT NULL,
  PRIMARY KEY (run_id, stage)
);
CREATE TABLE IF NOT EXISTS files (
  run_id  TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE,
  name    TEXT NOT NULL,
  content BLOB NOT NULL,
  PRIMARY KEY (run_id, name)
);
CREATE TABLE IF NOT EXISTS metrics (
  version     TEXT NOT NULL,
  id          TEXT NOT NULL,
  unit        TEXT NOT NULL,
  direction   TEXT NOT NULL,
  tool        TEXT NOT NULL,
  label       TEXT NOT NULL,
  description TEXT NOT NULL,
  name        TEXT NOT NULL,
  section     TEXT NOT NULL,
  builtin     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (version, id)
);

-- every run with its effective group (the `group` tag, else the automatic one)
CREATE VIEW IF NOT EXISTS v_runs AS
SELECT r.*,
       COALESCE((SELECT t.value FROM tags t WHERE t.run_id = r.run_id AND t.key = 'group'), r.auto_group) AS grp,
       EXISTS (SELECT 1 FROM metrics m WHERE m.version = ltrim(r.vmbench_version, 'v')) AS catalog_known
FROM runs r;

-- every raw value that counts: stages that failed, and stages listed in the run's `exclude` tag (a comma list,
-- spaces ignored, names matched exactly), are left out
CREATE VIEW IF NOT EXISTS v_values AS
SELECT v.run_id, r.label, r.grp, r.arch, r.nproc, r.ram_gb, r.cpu_model, r.vmbench_version,
       v.id, COALESCE(m.name, v.metric) AS name,
       substr(v.id, 1, instr(v.id || '.', '.') - 1) AS section,
       CASE WHEN m.unit IS NULL OR m.unit = 'varies' THEN v.unit ELSE m.unit END AS unit,
       m.direction, m.tool, v.round, v.value
FROM results v
JOIN v_runs r ON r.run_id = v.run_id
LEFT JOIN metrics m ON m.version = ltrim(r.vmbench_version, 'v') AND m.id = v.id
WHERE substr(v.id, 1, instr(v.id || '.', '.') - 1) NOT IN (SELECT f.stage FROM failed_stages f WHERE f.run_id = v.run_id)
  AND instr(',' || replace(COALESCE((SELECT t.value FROM tags t WHERE t.run_id = v.run_id AND t.key = 'exclude'), ''), ' ', '') || ',',
             ',' || substr(v.id, 1, instr(v.id || '.', '.') - 1) || ',') = 0;

-- per run and metric (SQLite has no median: benchlens itself computes medians and verdicts)
CREATE VIEW IF NOT EXISTS v_stats AS
SELECT run_id, label, grp, id, name, section, unit, direction,
       COUNT(*) AS n, AVG(value) AS mean, MIN(value) AS min, MAX(value) AS max
FROM v_values GROUP BY run_id, id;
"#;

/// What happened to one run on insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertOutcome {
    Inserted,
    /// The run was already in the database (same run id); only tags were applied.
    AlreadyPresent,
}

/// One run as listed.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub folder: String,
    pub label: String,
    pub group: String,
    pub arch: String,
    pub nproc: Option<i64>,
    pub ram_gb: Option<f64>,
    pub cpu_model: String,
    pub vmbench_version: String,
    pub start_utc: String,
    pub source: String,
    pub catalog_known: bool,
    pub failed_stages: Vec<String>,
    pub tags: BTreeMap<String, String>,
    pub values: i64,
    /// Names of the files the run kept (sorted), e.g. `report.md`; `RunDetail.files` has their sizes.
    pub kept_files: Vec<String>,
}

pub use benchlens_rules::compare::ValueRow;

/// A group: every run whose effective group (the `group` tag, else the automatic one) is the same.
#[derive(Debug, Clone, Serialize)]
pub struct GroupSummary {
    pub group: String,
    pub runs: usize,
    pub labels: Vec<String>,
    pub archs: Vec<String>,
    pub nproc: Vec<i64>,
    pub cpu_models: Vec<String>,
    pub vmbench_versions: Vec<String>,
    pub first_start_utc: String,
    pub last_start_utc: String,
}

/// A kept file of a run.
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub bytes: usize,
}

/// Everything about one run except its values.
#[derive(Debug, Clone, Serialize)]
pub struct RunDetail {
    #[serde(flatten)]
    pub summary: RunSummary,
    pub stages: String,
    pub end_utc: String,
    pub imported_at: String,
    pub pg_profile: String,
    pub web_cpus: String,
    pub meta: serde_json::Value,
    pub hardware: Option<serde_json::Value>,
    pub files: Vec<FileInfo>,
}

pub struct Store {
    conn: Connection,
}

/// Keep `-wal` and `-shm` when the last connection closes (`SQLITE_FCNTL_PERSIST_WAL`; the WAL is still
/// checkpointed). A read-only reader, such as Grafana on a `:ro` mount, cannot create those two files, so
/// without this it fails with "unable to open database file" whenever no benchlens process has the database
/// open (sqlite.org/wal.html, "Read-Only Databases"). Ignored where there is no file (in memory).
fn persist_wal(conn: &Connection) {
    let mut on: std::os::raw::c_int = 1;
    // SAFETY: the handle is valid while `conn` lives; PERSIST_WAL reads (and may write) the int it points to,
    // which outlives the call. The result code is ignored on purpose (SQLITE_NOTFOUND in memory).
    unsafe {
        rusqlite::ffi::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,
            (&mut on as *mut std::os::raw::c_int).cast(),
        );
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Store::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL").ok(); // not available in memory; harmless
        persist_wal(&conn);
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if v > DB_VERSION {
            return Err(Error::contract(format!(
                "database layout {v} is newer than this benchlens understands ({DB_VERSION}); upgrade benchlens"
            )));
        }
        // create or migrate in one transaction: tables are created if missing (data untouched); the views of
        // an older layout are dropped and created again
        let tx = conn.unchecked_transaction()?;
        if v < DB_VERSION {
            for view in VIEWS {
                tx.execute_batch(&format!("DROP VIEW IF EXISTS {view};"))?;
            }
        }
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(None, "user_version", DB_VERSION)?;
        tx.commit()?;
        let store = Store { conn };
        for v in catalog::embedded_versions() {
            store.put_catalog(&catalog::embedded(v).expect("embedded"), true)?;
        }
        Ok(store)
    }

    /// The raw connection, for tools and tests that need plain SQL.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Store a metric table. A built-in table replaces the stored one of the same version.
    pub fn put_catalog(&self, cat: &Catalog, builtin: bool) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM metrics WHERE version = ?1", [&cat.version])?;
        {
            let mut st = tx.prepare(
                "INSERT INTO metrics (version, id, unit, direction, tool, label, description, name, section, builtin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for m in cat.metrics.values() {
                st.execute(params![
                    cat.version,
                    m.id,
                    m.unit,
                    m.direction.as_str(),
                    m.tool,
                    m.label,
                    m.description,
                    m.name(),
                    m.section(),
                    builtin as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(cat.metrics.len())
    }

    /// Versions that have a metric table here, with whether it is built in.
    pub fn catalogs(&self) -> Result<Vec<(String, bool, i64)>> {
        let mut st = self.conn.prepare(
            "SELECT version, MAX(builtin), COUNT(*) FROM metrics GROUP BY version ORDER BY version",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn has_run(&self, run_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM runs WHERE run_id = ?1", [run_id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Insert a run with its values, or, if it is already here, only apply the tags.
    pub fn insert_run(
        &self,
        run: &RunData,
        source: &str,
        tags: &BTreeMap<String, String>,
    ) -> Result<(String, InsertOutcome)> {
        let run_id = run.run_id();
        let tx = self.conn.unchecked_transaction()?;
        let outcome = if self.has_run(&run_id)? {
            InsertOutcome::AlreadyPresent
        } else {
            let stages = match run.meta.get("stages") {
                Some(serde_json::Value::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            tx.execute(
                "INSERT INTO runs (run_id, folder, label, arch, nproc, ram_gb, cpu_model, vmbench_version, pg_profile,
                                   web_cpus, stages, start_utc, end_utc, auto_group, source, meta_json, hardware_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    run_id, run.folder, run.label(), run.arch(), run.nproc(), run.ram_gb(), run.cpu_model(),
                    run.vmbench_version(), run.meta_field("pg_profile"), run.meta_field("web_cpus"), stages,
                    run.start_utc(), run.meta_field("end_utc"), run.auto_group(), source, run.meta.to_string(),
                    run.hardware.as_ref().map(|h| h.to_string())
                ],
            )?;
            {
                let mut st = tx.prepare(
                    "INSERT INTO results (run_id, id, metric, round, value, unit) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for r in &run.results {
                    st.execute(params![run_id, r.id, r.metric, r.round, r.value, r.unit])?;
                }
                let mut st =
                    tx.prepare("INSERT INTO failed_stages (run_id, stage) VALUES (?1, ?2)")?;
                for s in &run.failed_stages {
                    st.execute(params![run_id, s])?;
                }
                let mut st =
                    tx.prepare("INSERT INTO files (run_id, name, content) VALUES (?1, ?2, ?3)")?;
                for (name, content) in &run.files {
                    st.execute(params![run_id, name, content])?;
                }
            }
            InsertOutcome::Inserted
        };
        set_tags_in(&tx, &run_id, tags)?;
        tx.commit()?;
        Ok((run_id, outcome))
    }

    /// Set tags on a run; an empty value removes the tag.
    pub fn set_tags(&self, run_id: &str, tags: &BTreeMap<String, String>) -> Result<()> {
        if !self.has_run(run_id)? {
            return Err(Error::NotFound(format!("no run {run_id}")));
        }
        let tx = self.conn.unchecked_transaction()?;
        set_tags_in(&tx, run_id, tags)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_run(&self, run_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM runs WHERE run_id = ?1", [run_id])?
            > 0)
    }

    pub fn list_runs(&self) -> Result<Vec<RunSummary>> {
        let mut st = self.conn.prepare(
            "SELECT run_id, folder, label, grp, arch, nproc, ram_gb, cpu_model, vmbench_version, start_utc, source,
                    catalog_known, (SELECT COUNT(*) FROM results v WHERE v.run_id = r.run_id)
             FROM v_runs r ORDER BY start_utc, label",
        )?;
        let mut runs: Vec<RunSummary> = st
            .query_map([], |r| {
                Ok(RunSummary {
                    run_id: r.get(0)?,
                    folder: r.get(1)?,
                    label: r.get(2)?,
                    group: r.get(3)?,
                    arch: r.get(4)?,
                    nproc: r.get(5)?,
                    ram_gb: r.get(6)?,
                    cpu_model: r.get(7)?,
                    vmbench_version: r.get(8)?,
                    start_utc: r.get(9)?,
                    source: r.get(10)?,
                    catalog_known: r.get::<_, i64>(11)? == 1,
                    failed_stages: Vec::new(),
                    tags: BTreeMap::new(),
                    values: r.get(12)?,
                    kept_files: Vec::new(),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut tag_st = self
            .conn
            .prepare("SELECT key, value FROM tags WHERE run_id = ?1 ORDER BY key")?;
        let mut fail_st = self
            .conn
            .prepare("SELECT stage FROM failed_stages WHERE run_id = ?1 ORDER BY stage")?;
        // every kept file name in one query (no query per run)
        let mut kept: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_st = self
            .conn
            .prepare("SELECT run_id, name FROM files ORDER BY run_id, name")?;
        let rows =
            file_st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (run_id, name) = row?;
            kept.entry(run_id).or_default().push(name);
        }
        for run in &mut runs {
            run.kept_files = kept.remove(&run.run_id).unwrap_or_default();
            run.tags = tag_st
                .query_map([&run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            run.failed_stages = fail_st
                .query_map([&run.run_id], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
        }
        Ok(runs)
    }

    /// One run as listed.
    pub fn run_summary(&self, run_id: &str) -> Result<RunSummary> {
        self.list_runs()?
            .into_iter()
            .find(|r| r.run_id == run_id)
            .ok_or_else(|| Error::NotFound(format!("no run {run_id}")))
    }

    /// One run with its meta.json, hardware.json and the list of kept files.
    pub fn run_detail(&self, run_id: &str) -> Result<RunDetail> {
        let summary = self.run_summary(run_id)?;
        let (stages, end_utc, imported_at, pg_profile, web_cpus, meta, hw): (String, String, String, String, String, String, Option<String>) =
            self.conn.query_row(
                "SELECT stages, end_utc, imported_at, pg_profile, web_cpus, meta_json, hardware_json FROM runs WHERE run_id = ?1",
                [run_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )?;
        let mut st = self
            .conn
            .prepare("SELECT name, length(content) FROM files WHERE run_id = ?1 ORDER BY name")?;
        let files = st
            .query_map([run_id], |r| {
                Ok(FileInfo {
                    name: r.get(0)?,
                    bytes: r.get::<_, i64>(1)? as usize,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(RunDetail {
            summary,
            stages,
            end_utc,
            imported_at,
            pg_profile,
            web_cpus,
            meta: serde_json::from_str(&meta)?,
            hardware: hw.map(|h| serde_json::from_str(&h)).transpose()?,
            files,
        })
    }

    /// A kept file of a run (meta.json, results.tsv, status.tsv, hardware.json, report.md, benchlens-tags.json).
    pub fn file(&self, run_id: &str, name: &str) -> Result<Vec<u8>> {
        self.conn
            .query_row(
                "SELECT content FROM files WHERE run_id = ?1 AND name = ?2",
                params![run_id, name],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("run {run_id} has no file {name}")))
    }

    /// Every group with what its runs have in common or not.
    pub fn list_groups(&self) -> Result<Vec<GroupSummary>> {
        let mut by: BTreeMap<String, GroupSummary> = BTreeMap::new();
        for r in self.list_runs()? {
            let g = by.entry(r.group.clone()).or_insert_with(|| GroupSummary {
                group: r.group.clone(),
                runs: 0,
                labels: vec![],
                archs: vec![],
                nproc: vec![],
                cpu_models: vec![],
                vmbench_versions: vec![],
                first_start_utc: r.start_utc.clone(),
                last_start_utc: r.start_utc.clone(),
            });
            g.runs += 1;
            let push = |v: &mut Vec<String>, x: &str| {
                if !x.is_empty() && !v.iter().any(|y| y == x) {
                    v.push(x.to_string())
                }
            };
            push(&mut g.labels, &r.label);
            push(&mut g.archs, &r.arch);
            push(&mut g.cpu_models, &r.cpu_model);
            push(&mut g.vmbench_versions, &r.vmbench_version);
            if let Some(n) = r.nproc {
                if !g.nproc.contains(&n) {
                    g.nproc.push(n);
                }
            }
            if r.start_utc < g.first_start_utc {
                g.first_start_utc = r.start_utc.clone();
            }
            if r.start_utc > g.last_start_utc {
                g.last_start_utc = r.start_utc.clone();
            }
        }
        Ok(by.into_values().collect())
    }

    /// The metric table of a vmbench version as stored (built-in or added).
    pub fn metrics(&self, version: &str) -> Result<Vec<benchlens_rules::catalog::Metric>> {
        let v = benchlens_rules::catalog::normalize_version(version);
        let mut st = self.conn.prepare(
            "SELECT id, unit, direction, tool, label, description FROM metrics WHERE version = ?1 ORDER BY id",
        )?;
        let rows = st.query_map([&v], |r| {
            Ok(benchlens_rules::catalog::Metric {
                id: r.get(0)?,
                unit: r.get(1)?,
                direction: Direction::parse(&r.get::<_, String>(2)?),
                tool: r.get(3)?,
                label: r.get(4)?,
                description: r.get(5)?,
            })
        })?;
        let out: Vec<_> = rows.collect::<std::result::Result<_, _>>()?;
        if out.is_empty() {
            return Err(Error::NotFound(format!("no metric table for vmbench {v}")));
        }
        Ok(out)
    }

    /// Number of runs.
    pub fn run_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))?)
    }

    /// Runs whose effective group is `group`.
    pub fn runs_in_group(&self, group: &str) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT run_id FROM v_runs WHERE grp = ?1 ORDER BY start_utc")?;
        let rows = st.query_map([group], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Runs whose effective group is `group` and whose tag `key` is exactly `value`.
    pub fn runs_in_group_tagged(&self, group: &str, key: &str, value: &str) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT r.run_id FROM v_runs r JOIN tags t ON t.run_id = r.run_id
             WHERE r.grp = ?1 AND t.key = ?2 AND t.value = ?3 ORDER BY r.start_utc",
        )?;
        let rows = st.query_map(params![group, key, value], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Every value that counts for these runs (failed and excluded stages are left out by `v_values`).
    pub fn values(&self, run_ids: &[String]) -> Result<Vec<ValueRow>> {
        self.values_where(run_ids, None)
    }

    /// The values of one metric for these runs: the rows of [`Store::values`] whose `id` is `metric`, in the
    /// same order, without loading the others.
    pub fn metric_values(&self, run_ids: &[String], metric: &str) -> Result<Vec<ValueRow>> {
        self.values_where(run_ids, Some(metric))
    }

    fn values_where(&self, run_ids: &[String], metric: Option<&str>) -> Result<Vec<ValueRow>> {
        let mut st = self.conn.prepare(
            "SELECT run_id, id, name, section, unit, direction, COALESCE(tool, ''), arch, round, value
             FROM v_values WHERE run_id = ?1 AND (?2 IS NULL OR id = ?2) ORDER BY id, round",
        )?;
        let mut out = Vec::new();
        for id in run_ids {
            let rows = st.query_map(params![id, metric], |r| {
                Ok(ValueRow {
                    run_id: r.get(0)?,
                    id: r.get(1)?,
                    name: r.get(2)?,
                    section: r.get(3)?,
                    unit: r.get(4)?,
                    direction: r.get::<_, Option<String>>(5)?.map(|d| Direction::parse(&d)),
                    tool: r.get(6)?,
                    arch: r.get(7)?,
                    round: r.get(8)?,
                    value: r.get(9)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    /// Resolve a run reference: a run id or a unique prefix of one (4+ characters), a folder name or a label.
    pub fn find_run(&self, reference: &str) -> Result<String> {
        let mut st = self.conn.prepare(
            "SELECT run_id FROM runs WHERE run_id = ?1 OR folder = ?1 OR label = ?1
             OR (length(?1) >= 4 AND substr(run_id, 1, length(?1)) = ?1)",
        )?;
        let ids: Vec<String> = st
            .query_map([reference], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        match ids.len() {
            1 => Ok(ids.into_iter().next().unwrap()),
            0 => Err(Error::NotFound(format!(
                "no run matches {reference:?} (try `benchlens runs`)"
            ))),
            n => Err(Error::Ambiguous(format!(
                "{reference:?} matches {n} runs; use a run id, or group:NAME to pool them"
            ))),
        }
    }

    /// Label and group of a run, for display.
    pub fn run_names(&self, run_id: &str) -> Result<(String, String, String)> {
        Ok(self.conn.query_row(
            "SELECT label, folder, grp FROM v_runs WHERE run_id = ?1",
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }
}

/// A tag key is 1-64 letters, digits, `_`, `-` or `.`.
pub fn valid_tag_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Longest tag value, in bytes.
pub const MAX_TAG_VALUE: usize = 1024;

/// Check one tag: the key follows [`valid_tag_key`], the value is one line of at most [`MAX_TAG_VALUE`] bytes
/// (an empty value means "remove"). The one rule for every route that sets tags.
pub fn check_tag(key: &str, value: &str) -> Result<()> {
    if !valid_tag_key(key) {
        return Err(Error::contract(format!(
            "tag key {key:?}: use 1-64 letters, digits, _ - or ."
        )));
    }
    if value.len() > MAX_TAG_VALUE || value.contains(['\n', '\r']) {
        return Err(Error::contract(format!(
            "tag {key}: the value must be one line of at most {MAX_TAG_VALUE} bytes"
        )));
    }
    Ok(())
}

/// Read `KEY=VALUE` (key and value trimmed) and check it with [`check_tag`].
pub fn parse_tag(text: &str) -> Result<(String, String)> {
    let (k, v) = text
        .split_once('=')
        .ok_or_else(|| Error::contract(format!("tag {text:?}: write KEY=VALUE")))?;
    let (k, v) = (k.trim(), v.trim());
    check_tag(k, v)?;
    Ok((k.to_string(), v.to_string()))
}

fn set_tags_in(
    tx: &rusqlite::Transaction,
    run_id: &str,
    tags: &BTreeMap<String, String>,
) -> Result<()> {
    for (k, v) in tags {
        check_tag(k, v)?;
        if v.is_empty() {
            tx.execute(
                "DELETE FROM tags WHERE run_id = ?1 AND key = ?2",
                params![run_id, k],
            )?;
        } else {
            tx.execute(
                "INSERT INTO tags (run_id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT (run_id, key) DO UPDATE SET value = excluded.value",
                params![run_id, k, v],
            )?;
        }
    }
    Ok(())
}
