//! Bringing vmbench results in: a result folder, a directory of result folders, or a `.tar` / `.tar.gz`
//! archive holding one or more of them.
//!
//! Archives are read in memory and never extracted to disk. An entry with an absolute path, a `..`
//! component, a link or device type, or an oversized body makes the whole archive refused. Only the files of
//! the data contract are kept; everything else in a folder is ignored.

use crate::contract::{RunData, KEPT_FILES, MAX_FILE_BYTES};
use crate::store::{InsertOutcome, Store};
use crate::{Error, Result};
use flate2::read::GzDecoder;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Component, Path};

/// Total bytes read from one archive before it is refused.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
/// Entries read from one archive before it is refused.
pub const MAX_ARCHIVE_ENTRIES: usize = 200_000;

/// Tags for many runs at once: a TSV whose `run` column holds the result folder name; every other column is
/// a tag (empty cells are skipped).
#[derive(Debug, Default, Clone)]
pub struct Registry {
    pub by_folder: BTreeMap<String, BTreeMap<String, String>>,
}

impl Registry {
    pub fn parse(text: &str) -> Result<Registry> {
        let mut lines = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
        let header: Vec<&str> = lines
            .next()
            .ok_or_else(|| Error::contract("registry is empty"))?
            .split('\t')
            .collect();
        let run_col = header.iter().position(|h| *h == "run").ok_or_else(|| {
            Error::contract("registry needs a `run` column with the result folder names")
        })?;
        let mut by_folder = BTreeMap::new();
        for line in lines {
            let cells: Vec<&str> = line.split('\t').collect();
            let Some(run) = cells
                .get(run_col)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let mut tags = BTreeMap::new();
            for (i, h) in header.iter().enumerate() {
                if i == run_col {
                    continue;
                }
                if let Some(v) = cells.get(i).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    tags.insert(h.trim().to_string(), v.to_string());
                }
            }
            by_folder.insert(run.to_string(), tags);
        }
        Ok(Registry { by_folder })
    }
}

/// How to import.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Where the run came from: `cli`, `inbox`, `upload`, `api`.
    pub source: String,
    /// Tags for every imported run; they win over the registry and benchlens-tags.json.
    pub tags: BTreeMap<String, String>,
    pub registry: Option<Registry>,
    /// Read and check only; write nothing.
    pub dry_run: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            source: "cli".into(),
            tags: BTreeMap::new(),
            registry: None,
            dry_run: false,
        }
    }
}

/// What happened to one folder.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub folder: String,
    pub run_id: Option<String>,
    pub outcome: Outcome,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Imported,
    AlreadyPresent,
    WouldImport,
    Failed,
}

/// Import a folder, a directory of folders, or an archive. Problems with one folder are reported and the rest
/// still imported; an unsafe archive is refused as a whole (returned as Err).
pub fn import_path(store: &Store, path: &Path, opts: &ImportOptions) -> Result<Vec<ImportReport>> {
    let md = fs::symlink_metadata(path)
        .map_err(|e| Error::contract(format!("{}: {e}", path.display())))?;
    let runs: Vec<(String, Result<RunData>)> = if md.is_file() {
        read_archive(path)?
    } else if md.is_dir() {
        if path.join("meta.json").is_file() {
            vec![(dir_name(path), RunData::from_dir(path))]
        } else {
            let mut kids: Vec<_> = fs::read_dir(path)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                        && e.path().join("meta.json").is_file()
                })
                .map(|e| e.path())
                .collect();
            kids.sort();
            if kids.is_empty() {
                return Err(Error::contract(format!(
                    "{}: no vmbench result folder here (a folder with meta.json, or folders that hold one)",
                    path.display()
                )));
            }
            kids.iter()
                .map(|k| (dir_name(k), RunData::from_dir(k)))
                .collect()
        }
    } else {
        return Err(Error::contract(format!(
            "{}: not a folder or an archive (links are not followed)",
            path.display()
        )));
    };
    Ok(runs
        .into_iter()
        .map(|(folder, run)| import_one(store, folder, run, opts))
        .collect())
}

fn import_one(
    store: &Store,
    folder: String,
    run: Result<RunData>,
    opts: &ImportOptions,
) -> ImportReport {
    let fail = |folder: String, e: Error| ImportReport {
        folder,
        run_id: None,
        outcome: Outcome::Failed,
        message: e.to_string(),
    };
    let run = match run {
        Ok(r) => r,
        Err(e) => return fail(folder, e),
    };
    let mut tags = opts
        .registry
        .as_ref()
        .and_then(|r| r.by_folder.get(&run.folder))
        .cloned()
        .unwrap_or_default();
    tags.extend(run.tags.clone());
    tags.extend(opts.tags.clone());
    let run_id = run.run_id();
    let note = if run.failed_stages.is_empty() {
        String::new()
    } else {
        format!(
            " (failed stages left out: {})",
            run.failed_stages
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if opts.dry_run {
        let present = store.has_run(&run_id).unwrap_or(false);
        return ImportReport {
            folder: run.folder.clone(),
            run_id: Some(run_id),
            outcome: if present {
                Outcome::AlreadyPresent
            } else {
                Outcome::WouldImport
            },
            message: format!(
                "{} values, vmbench {}{note}",
                run.results.len(),
                run.vmbench_version()
            ),
        };
    }
    match store.insert_run(&run, &opts.source, &tags) {
        Ok((id, InsertOutcome::Inserted)) => ImportReport {
            folder: run.folder.clone(),
            run_id: Some(id),
            outcome: Outcome::Imported,
            message: format!(
                "{} values, vmbench {}{note}",
                run.results.len(),
                run.vmbench_version()
            ),
        },
        Ok((id, InsertOutcome::AlreadyPresent)) => ImportReport {
            folder: run.folder.clone(),
            run_id: Some(id),
            outcome: Outcome::AlreadyPresent,
            message: if tags.is_empty() {
                "already imported".into()
            } else {
                "already imported; tags updated".into()
            },
        },
        Err(e) => fail(run.folder.clone(), e),
    }
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

/// Read every result folder in a .tar or .tar.gz (detected from the content, not the name).
pub fn read_archive(path: &Path) -> Result<Vec<(String, Result<RunData>)>> {
    let mut f = fs::File::open(path)?;
    let mut magic = [0u8; 2];
    let n = f.read(&mut magic)?;
    drop(f);
    let file = BufReader::new(fs::File::open(path)?);
    let stem = dir_name(path)
        .trim_end_matches(".gz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".tar")
        .to_string();
    if n == 2 && magic == [0x1f, 0x8b] {
        read_tar(GzDecoder::new(file), &stem)
    } else {
        read_tar(file, &stem)
    }
}

/// Read a tar stream; see the module notes for what is refused.
pub fn read_tar<R: Read>(reader: R, stem: &str) -> Result<Vec<(String, Result<RunData>)>> {
    let mut ar = tar::Archive::new(reader);
    let mut folders: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    let (mut total, mut count) = (0u64, 0usize);
    for entry in ar.entries()? {
        let mut entry = entry?;
        count += 1;
        let path = entry.path()?.into_owned();
        let shown = path.display().to_string();
        let unsafe_entry = |why: &str| Error::UnsafeArchive {
            path: shown.clone(),
            why: why.to_string(),
        };
        if count > MAX_ARCHIVE_ENTRIES {
            return Err(unsafe_entry("too many entries"));
        }
        for c in path.components() {
            match c {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(unsafe_entry("absolute path or `..` component")),
            }
        }
        use tar::EntryType as T;
        match entry.header().entry_type() {
            T::Regular | T::Continuous => {}
            T::Directory | T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink => {
                continue
            }
            _ => {
                return Err(unsafe_entry(
                    "links, devices and other special entries are not allowed",
                ))
            }
        }
        let size = entry.header().size()?;
        if size > MAX_FILE_BYTES {
            return Err(unsafe_entry("file too large"));
        }
        total += size;
        if total > MAX_ARCHIVE_BYTES {
            return Err(unsafe_entry("archive too large"));
        }
        let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if !KEPT_FILES.contains(&name.as_str()) {
            continue; // logs and raw tool output are not needed
        }
        let parent: Vec<String> = path
            .parent()
            .map(|p| {
                p.components()
                    .filter_map(|c| match c {
                        Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let key = if parent.is_empty() {
            stem.to_string()
        } else {
            parent.join("/")
        };
        let mut buf = Vec::with_capacity(size as usize);
        entry
            .by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut buf)?;
        if buf.len() as u64 > MAX_FILE_BYTES {
            return Err(unsafe_entry("file larger than its header says"));
        }
        folders.entry(key).or_default().insert(name, buf);
    }
    let folders: Vec<_> = folders
        .into_iter()
        .filter(|(_, files)| files.contains_key("meta.json"))
        .collect();
    if folders.is_empty() {
        return Err(Error::contract(
            "archive holds no vmbench result folder (no meta.json)",
        ));
    }
    Ok(folders
        .into_iter()
        .map(|(key, files)| {
            let folder = key.rsplit('/').next().unwrap_or(&key).to_string();
            (folder.clone(), RunData::from_files(&folder, files))
        })
        .collect())
}
