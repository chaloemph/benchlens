//! What benchlens reads from a vmbench result folder: the data contract with vmbench.
//!
//! Only `meta.json`, `results.tsv`, `status.tsv` and `hardware.json` are relied on (`report.md` and a
//! `benchlens-tags.json` are kept when present). `meta.json` must carry a `"schema"` that benchlens knows;
//! a folder with another schema is refused instead of being read wrongly.

use crate::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// meta.json `"schema"` values this version of benchlens understands.
pub const SUPPORTED_SCHEMAS: &[i64] = &[1];
/// Files a folder must have.
pub const REQUIRED_FILES: &[&str] = &["meta.json", "results.tsv", "status.tsv"];
/// Files benchlens copies from a folder; nothing else is read.
pub const KEPT_FILES: &[&str] = &[
    "meta.json",
    "results.tsv",
    "status.tsv",
    "hardware.json",
    "report.md",
    TAGS_FILE,
];
/// Optional file in a result folder with tags for benchlens: `{"set": "poc", "round": "2"}`.
pub const TAGS_FILE: &str = "benchlens-tags.json";
/// Largest single file accepted (results.tsv of a long run is well under 1 MB).
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// One line of results.tsv.
#[derive(Debug, Clone)]
pub struct ResultRow {
    pub id: String,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub round: i64,
}

/// A vmbench result folder, read and checked.
#[derive(Debug, Clone)]
pub struct RunData {
    /// Folder name, e.g. `x86-high-r2-20261119-145026`.
    pub folder: String,
    pub meta: Value,
    pub results: Vec<ResultRow>,
    /// Stages with a FAIL line in status.tsv; their values are not used.
    pub failed_stages: BTreeSet<String>,
    pub hardware: Option<Value>,
    /// Tags from benchlens-tags.json, if the folder has one.
    pub tags: BTreeMap<String, String>,
    /// The kept files as they were, by name.
    pub files: BTreeMap<String, Vec<u8>>,
}

impl RunData {
    /// Read a result folder on disk.
    pub fn from_dir(path: &Path) -> Result<RunData> {
        let folder = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let mut files = BTreeMap::new();
        for name in KEPT_FILES {
            let p = path.join(name);
            let Ok(md) = fs::symlink_metadata(&p) else {
                continue;
            };
            if !md.is_file() {
                continue; // a symlink or directory is never followed
            }
            if md.len() > MAX_FILE_BYTES {
                return Err(Error::contract(format!(
                    "{folder}: {name} is larger than {MAX_FILE_BYTES} bytes"
                )));
            }
            files.insert(name.to_string(), fs::read(&p)?);
        }
        RunData::from_files(&folder, files)
    }

    /// Build from file contents already in memory (from a directory or an archive).
    pub fn from_files(folder: &str, files: BTreeMap<String, Vec<u8>>) -> Result<RunData> {
        let missing: Vec<&str> = REQUIRED_FILES
            .iter()
            .copied()
            .filter(|f| !files.contains_key(*f))
            .collect();
        if !missing.is_empty() {
            return Err(Error::contract(format!(
                "{folder}: missing {} (is this a vmbench result folder?)",
                missing.join(", ")
            )));
        }
        let meta: Value = serde_json::from_slice(&files["meta.json"])
            .map_err(|e| Error::contract(format!("{folder}: meta.json is not valid JSON: {e}")))?;
        let schema = meta.get("schema").and_then(Value::as_i64);
        if !schema.is_some_and(|s| SUPPORTED_SCHEMAS.contains(&s)) {
            return Err(Error::contract(format!(
                "{folder}: meta.json schema {} is not supported (supported: {SUPPORTED_SCHEMAS:?}); \
                 this vmbench version needs a newer benchlens",
                schema.map(|s| s.to_string()).unwrap_or_else(|| "missing".into())
            )));
        }
        let results = parse_results(&files["results.tsv"])?;
        let failed_stages = parse_failed(&files["status.tsv"])?;
        let hardware = match files.get("hardware.json") {
            Some(b) => Some(serde_json::from_slice(b).map_err(|e| {
                Error::contract(format!("{folder}: hardware.json is not valid JSON: {e}"))
            })?),
            None => None,
        };
        let tags = match files.get(TAGS_FILE) {
            Some(b) => {
                parse_tags(b).map_err(|e| Error::contract(format!("{folder}: {TAGS_FILE}: {e}")))?
            }
            None => BTreeMap::new(),
        };
        Ok(RunData {
            folder: folder.to_string(),
            meta,
            results,
            failed_stages,
            hardware,
            tags,
            files,
        })
    }

    fn meta_str(&self, key: &str) -> String {
        match self.meta.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }
    pub fn label(&self) -> String {
        let l = self.meta_str("label");
        if l.is_empty() {
            self.folder.clone()
        } else {
            l
        }
    }
    pub fn arch(&self) -> String {
        self.meta_str("arch")
    }
    pub fn nproc(&self) -> Option<i64> {
        match self.meta.get("nproc") {
            Some(Value::Number(n)) => n.as_i64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn ram_gb(&self) -> Option<f64> {
        match self.meta.get("ram_gb") {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn vmbench_version(&self) -> String {
        self.meta_str("vmbench_version")
    }
    pub fn start_utc(&self) -> String {
        self.meta_str("start_utc")
    }
    pub fn meta_field(&self, key: &str) -> String {
        self.meta_str(key)
    }

    /// Stable id of the run: the same run imported again, by any route, gets the same id.
    pub fn run_id(&self) -> String {
        let mut h = Sha256::new();
        if self.start_utc().is_empty() {
            h.update(&self.files["results.tsv"]); // old folders without a start time: the values themselves
        } else {
            h.update(format!(
                "{}\n{}\n{}\n{:?}",
                self.label(),
                self.start_utc(),
                self.arch(),
                self.nproc()
            ));
        }
        hex::encode(&h.finalize()[..8])
    }

    /// Readable CPU name. vmbench <= 0.2.3 reports model "-" on some ARM CPUs, so fall back to the vendor.
    pub fn cpu_model(&self) -> String {
        let cpu = self.hardware.as_ref().and_then(|h| h.get("cpu"));
        let get = |k: &str| {
            cpu.and_then(|c| c.get(k))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let model = get("model");
        if !model.is_empty() && model != "-" {
            model
        } else {
            get("vendor")
        }
    }

    /// Default group when no `group` tag is given: the label without round / rerun suffixes, plus the hardware,
    /// so `x86-std` and `x86-std-r2` on the same machine land together and two people's `x86-std` do not.
    pub fn auto_group(&self) -> String {
        let label = strip_round_suffix(&self.label());
        let ram = self
            .ram_gb()
            .map(|r| format!(", {r} GB"))
            .unwrap_or_default();
        let cpu = self.cpu_model();
        let n = self
            .nproc()
            .map(|n| format!("{n} vCPU"))
            .unwrap_or_default();
        format!(
            "{label} ({}{}{ram})",
            if cpu.is_empty() {
                String::new()
            } else {
                format!("{cpu}, ")
            },
            n
        )
    }
}

/// `x86-std-r2` -> `x86-std`, `x86-std-web` -> `x86-std`; only trailing round / rerun words are dropped.
pub fn strip_round_suffix(label: &str) -> String {
    let mut parts: Vec<&str> = label.split('-').collect();
    while parts.len() > 1 {
        let last = parts[parts.len() - 1].to_ascii_lowercase();
        let roundish = (last.starts_with('r')
            && last.len() > 1
            && last[1..].chars().all(|c| c.is_ascii_digit()))
            || (last.starts_with("round") && last[5..].chars().all(|c| c.is_ascii_digit()))
            || matches!(last.as_str(), "web" | "rerun" | "retry");
        if !roundish {
            break;
        }
        parts.pop();
    }
    parts.join("-")
}

fn tsv(bytes: &[u8]) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .flexible(true)
        .has_headers(false)
        .quoting(false)
        .from_reader(bytes)
}

/// results.tsv: header `id metric value unit round`; lines whose value is not a number are skipped,
/// as report.py does.
fn parse_results(bytes: &[u8]) -> Result<Vec<ResultRow>> {
    let mut out = Vec::new();
    for rec in tsv(bytes).records() {
        let r = rec?;
        if r.len() < 5 || (&r[0] == "id" && &r[2] == "value") {
            continue;
        }
        if &r[0] == "row" {
            return Err(Error::contract("results.tsv uses the pre-0.1 format without metric ids; rerun with vmbench 0.2 or later"));
        }
        let Ok(value) = r[2].trim().parse::<f64>() else {
            continue;
        };
        if !value.is_finite() {
            continue;
        }
        out.push(ResultRow {
            id: r[0].to_string(),
            metric: r[1].to_string(),
            value,
            unit: r[3].to_string(),
            round: r[4].trim().parse().unwrap_or(1),
        });
    }
    Ok(out)
}

/// status.tsv: `stage step result`; the result may carry a reason ("FAIL see web.log").
fn parse_failed(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for rec in tsv(bytes).records() {
        let r = rec?;
        if r.len() >= 3 && r[2].split(' ').next() == Some("FAIL") {
            out.insert(r[0].to_string());
        }
    }
    Ok(out)
}

fn parse_tags(bytes: &[u8]) -> std::result::Result<BTreeMap<String, String>, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("must be a JSON object of key: value")?;
    let mut out = BTreeMap::new();
    for (k, v) in obj {
        let val = match v {
            Value::String(s) => s.clone(),
            Value::Number(_) | Value::Bool(_) => v.to_string(),
            _ => {
                return Err(format!(
                    "tag {k}: value must be text, a number or true/false"
                ))
            }
        };
        out.insert(k.clone(), val);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_suffix() {
        assert_eq!(strip_round_suffix("x86-std-r2"), "x86-std");
        assert_eq!(strip_round_suffix("x86-std-web"), "x86-std");
        assert_eq!(strip_round_suffix("arm64-high"), "arm64-high");
        assert_eq!(strip_round_suffix("r2"), "r2");
        assert_eq!(strip_round_suffix("db-round3"), "db");
        assert_eq!(strip_round_suffix("x86-rack"), "x86-rack");
    }

    #[test]
    fn status_reason_counts_as_fail() {
        let s = parse_failed(
            b"stage\tstep\tresult\nweb\tloopback\tFAIL see web.log\nmem\tx\tSKIP one NUMA node\n",
        )
        .unwrap();
        assert_eq!(s.into_iter().collect::<Vec<_>>(), vec!["web".to_string()]);
    }
}
