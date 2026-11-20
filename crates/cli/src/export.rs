//! `benchlens export`: a comparison as one self-contained HTML file (built by `benchlens_server::export`, the
//! same code as `GET /api/export.html`), or the values of sides as JSON or CSV.

use anyhow::{bail, Context, Result};
use benchlens_core::compare::{self, stats, Side, Stat, ValueRow};
use benchlens_core::store::Store;
use benchlens_server::export as html;
use clap::Subcommand;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ExportCmd {
    /// One HTML file with a comparison that opens from disk, offline: baseline and sides can be changed among
    /// the exported ones, verdicts computed by the same rule (WebAssembly).
    Html {
        /// Baseline: run id, label, folder or group:NAME.
        #[arg(long)]
        base: String,
        /// A side to compare with the baseline (repeat for more).
        #[arg(long = "other", required = true, value_name = "REF")]
        others: Vec<String>,
        /// Output file.
        #[arg(short, long, default_value = "report.html")]
        output: PathBuf,
        /// Title shown in the page (default: "BASE vs OTHER, ...").
        #[arg(long)]
        title: Option<String>,
    },
    /// Every value of each side plus its statistics per metric, as JSON (stdout without -o).
    Json {
        #[arg(required = true, value_name = "REF")]
        refs: Vec<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Every value of each side, long format: side,run_id,label,id,name,section,unit,direction,round,value
    /// (stdout without -o).
    Csv {
        #[arg(required = true, value_name = "REF")]
        refs: Vec<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

/// Columns of `benchlens export csv`, in order.
pub const CSV_COLUMNS: [&str; 10] = [
    "side",
    "run_id",
    "label",
    "id",
    "name",
    "section",
    "unit",
    "direction",
    "round",
    "value",
];

/// Shown as the data source of an export: the database file name (not its full path).
fn source_name(db: &Path) -> String {
    db.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| db.display().to_string())
}

pub fn run(db: &Path, cmd: ExportCmd) -> Result<()> {
    let store = Store::open(db).with_context(|| format!("opening {}", db.display()))?;
    match cmd {
        ExportCmd::Html {
            base,
            others,
            output,
            title,
        } => {
            let data = html::collect(&store, &base, &others, &source_name(db), title.as_deref())?;
            let page = html::render_html(&data)?;
            fs::write(&output, &page).with_context(|| format!("writing {}", output.display()))?;
            let values: usize = data.sides.iter().map(|s| s.values.len()).sum();
            eprintln!(
                "wrote {} ({} KB): {} sides, {} runs, {} values",
                output.display(),
                page.len().div_ceil(1024),
                data.sides.len(),
                data.runs.len(),
                values
            );
        }
        ExportCmd::Json { refs, output } => {
            let doc = json_export(&store, &refs, &source_name(db))?;
            let mut text = serde_json::to_string_pretty(&doc)?;
            text.push('\n');
            write_out(output.as_deref(), text.as_bytes())?;
        }
        ExportCmd::Csv { refs, output } => {
            let text = csv_export(&store, &refs)?;
            write_out(output.as_deref(), text.as_bytes())?;
        }
    }
    Ok(())
}

fn write_out(path: Option<&Path>, bytes: &[u8]) -> Result<()> {
    match path {
        Some(p) => fs::write(p, bytes).with_context(|| format!("writing {}", p.display())),
        None => {
            std::io::stdout().write_all(bytes)?;
            Ok(())
        }
    }
}

/// One side in `benchlens export json`.
#[derive(Serialize)]
pub struct JsonSide {
    /// `archs` filled from the values, as in `benchlens compare`.
    pub side: Side,
    /// Median, CV, min, max and count per metric (the numbers `compare` judges).
    pub stats: Vec<Stat>,
    /// Values of a vmbench version without a metric table: in `values`, not in `stats`.
    pub unjudged: usize,
    pub values: Vec<ValueRow>,
}

/// The document `benchlens export json` writes.
#[derive(Serialize)]
pub struct JsonExport {
    pub benchlens: &'static str,
    pub exported_at: String,
    pub source: String,
    pub sides: Vec<JsonSide>,
}

fn load_all(store: &Store, refs: &[String]) -> Result<Vec<(Side, Vec<ValueRow>)>> {
    if refs.is_empty() {
        bail!("name at least one side (run id, label, folder or group:NAME)");
    }
    refs.iter()
        .map(|r| compare::load(store, r).map_err(Into::into))
        .collect()
}

pub fn json_export(store: &Store, refs: &[String], source: &str) -> Result<JsonExport> {
    let sides = load_all(store, refs)?
        .into_iter()
        .map(|(mut side, values)| {
            side.archs = values.iter().map(|v| v.arch.clone()).collect();
            let (st, unjudged) = stats(&values);
            JsonSide {
                side,
                stats: st.into_values().collect(),
                unjudged,
                values,
            }
        })
        .collect();
    Ok(JsonExport {
        benchlens: env!("CARGO_PKG_VERSION"),
        exported_at: html::utc_now(),
        source: source.into(),
        sides,
    })
}

/// One CSV field: quoted when it holds `,` `"` or a line break; a text field starting with `= + - @`, tab or
/// CR gets a leading `'` so spreadsheets do not run it as a formula.
fn csv_field(out: &mut String, s: &str, text: bool) {
    let guard = text && s.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let s = if guard {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&s.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(&s);
    }
}

pub fn csv_export(store: &Store, refs: &[String]) -> Result<String> {
    let sides = load_all(store, refs)?;
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    for (side, _) in &sides {
        for id in &side.run_ids {
            if !labels.contains_key(id) {
                labels.insert(id.clone(), store.run_names(id)?.0);
            }
        }
    }
    let mut out = CSV_COLUMNS.join(",");
    out.push_str("\r\n");
    for (side, values) in &sides {
        for v in values {
            let direction = match v.direction {
                Some(d) => serde_json::to_value(d)?
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                None => String::new(),
            };
            let fields: [(&str, bool); 10] = [
                (&side.reference, true),
                (&v.run_id, true),
                (
                    labels.get(&v.run_id).map(String::as_str).unwrap_or(""),
                    true,
                ),
                (&v.id, true),
                (&v.name, true),
                (&v.section, true),
                (&v.unit, true),
                (&direction, true),
                (&v.round.to_string(), false),
                (&v.value.to_string(), false),
            ];
            for (i, (f, text)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                csv_field(&mut out, f, *text);
            }
            out.push_str("\r\n");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use benchlens_core::testing::fixture_store;

    const X86: &str = "group:x86-high";
    const ARM: &str = "group:arm64-high";

    /// A minimal RFC 4180 reader for the tests.
    fn parse_csv(text: &str) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let (mut row, mut field, mut quoted, mut chars) =
            (Vec::new(), String::new(), false, text.chars().peekable());
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                ('"', _) => quoted = !quoted,
                (',', false) => row.push(std::mem::take(&mut field)),
                ('\r', false) => {}
                ('\n', false) => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                (c, _) => field.push(c),
            }
        }
        rows
    }

    #[test]
    fn csv_columns_and_rows() {
        let store = fixture_store(true);
        let refs = vec![X86.to_string(), ARM.to_string()];
        let rows = parse_csv(&csv_export(&store, &refs).unwrap());
        assert_eq!(rows[0], CSV_COLUMNS);
        let mut want = 0;
        for r in &refs {
            let (side, values) = compare::load(&store, r).unwrap();
            want += values.len();
            let mine: Vec<&Vec<String>> = rows[1..].iter().filter(|x| x[0] == *r).collect();
            assert_eq!(mine.len(), values.len(), "{r}");
            for (row, v) in mine.iter().zip(&values) {
                assert_eq!(row.len(), 10);
                assert!(side.run_ids.contains(&row[1]));
                assert_eq!(row[2], store.run_names(&v.run_id).unwrap().0);
                assert_eq!(
                    (&row[3], &row[4], &row[5], &row[6]),
                    (&v.id, &v.name, &v.section, &v.unit)
                );
                assert!(["higher", "lower", "none", ""].contains(&row[7].as_str()));
                assert_eq!(row[8].parse::<i64>().unwrap(), v.round);
                assert_eq!(row[9].parse::<f64>().unwrap(), v.value);
            }
        }
        assert_eq!(rows.len(), 1 + want);
    }

    #[test]
    fn csv_quotes_and_defuses_formulas() {
        let mut out = String::new();
        csv_field(&mut out, "a,\"b\"\nc", true);
        assert_eq!(out, "\"a,\"\"b\"\"\nc\"");
        out.clear();
        csv_field(&mut out, "=HYPERLINK(\"x\")", true);
        assert_eq!(out, "\"'=HYPERLINK(\"\"x\"\")\"");
        out.clear();
        csv_field(&mut out, "-1.5", false);
        assert_eq!(out, "-1.5");
    }

    #[test]
    fn json_sides_values_and_stats() {
        let store = fixture_store(true);
        let refs = vec![X86.to_string(), ARM.to_string()];
        let doc = serde_json::to_value(json_export(&store, &refs, "test.db").unwrap()).unwrap();
        assert_eq!(doc["source"], "test.db");
        let sides = doc["sides"].as_array().unwrap();
        assert_eq!(sides.len(), 2);
        let c = compare::compare(&store, X86, &[ARM.to_string()]).unwrap();
        for (i, r) in refs.iter().enumerate() {
            let (side, values) = compare::load(&store, r).unwrap();
            let s = &sides[i];
            assert_eq!(s["side"]["reference"], r.as_str());
            assert_eq!(s["side"]["run_ids"], serde_json::json!(side.run_ids));
            assert!(!s["side"]["archs"].as_array().unwrap().is_empty());
            assert_eq!(s["values"].as_array().unwrap().len(), values.len());
            for k in [
                "id",
                "name",
                "section",
                "unit",
                "direction",
                "tool",
                "n",
                "median",
                "cv",
                "min",
                "max",
            ] {
                assert!(s["stats"][0].get(k).is_some(), "stats field {k}");
            }
            // the medians are the ones compare judges
            for row in &c.rows {
                let st = s["stats"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|x| x["id"] == row.id.as_str())
                    .unwrap();
                let want = if i == 0 { row.base } else { row.value };
                assert_eq!(st["median"].as_f64().unwrap(), want, "{}", row.id);
            }
        }
    }

    #[test]
    fn unknown_side_is_an_error() {
        let store = fixture_store(true);
        assert!(csv_export(&store, &["group:nope".into()]).is_err());
        assert!(json_export(&store, &[], "x").is_err());
    }
}
