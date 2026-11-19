//! The inbox: a folder where result folders and archives are dropped and imported automatically.
//!
//! Every scan looks at the top-level entries of the inbox: a result folder (a directory holding
//! `meta.json`), a directory of result folders, or a regular `.tar` / `.tar.gz` / `.tgz` file. Dotfiles,
//! links, `done/`, `failed/` and partial uploads (`.part`, `.tmp`, `.partial`) are left alone.
//!
//! An entry is imported only once its total size and newest modification time stayed the same between two
//! consecutive scans, so a folder or archive that is still being copied is not read half-way. After the
//! import the entry is renamed into `done/<timestamp>-<name>` (every folder imported or already present) or
//! `failed/<timestamp>-<name>` with a `<timestamp>-<name>.reason.txt` next to it. Nothing is overwritten and
//! nothing is deleted.
//!
//! [`watch`] scans by polling every few seconds; there is no filesystem-event watcher, since a scan is
//! cheap and the stability rule needs a second look after a delay anyway.

use crate::import::{import_path, ImportOptions, ImportReport, Outcome};
use crate::store::Store;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default time between two scans.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);
/// Where successfully imported entries go.
pub const DONE_DIR: &str = "done";
/// Where entries that could not be imported go, with a `.reason.txt`.
pub const FAILED_DIR: &str = "failed";

const PARTIAL_SUFFIXES: [&str; 3] = [".part", ".tmp", ".partial"];
const ARCHIVE_SUFFIXES: [&str; 3] = [".tar", ".tar.gz", ".tgz"];

/// Where the inbox is and how often to scan it.
#[derive(Debug, Clone)]
pub struct InboxOptions {
    pub dir: PathBuf,
    pub interval: Duration,
}

impl InboxOptions {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        InboxOptions {
            dir: dir.into(),
            interval: DEFAULT_INTERVAL,
        }
    }
}

/// What a scan did with one entry.
#[derive(Debug, Clone)]
pub enum InboxEvent {
    /// Seen for the first time; imported once it stops changing.
    Waiting { name: String },
    /// Every folder imported or already present; the entry was moved to `moved_to`.
    Done {
        name: String,
        moved_to: PathBuf,
        reports: Vec<ImportReport>,
    },
    /// A folder failed or the archive was refused; the entry was moved to `moved_to` and `reason` written to
    /// `reason_file`.
    Failed {
        name: String,
        moved_to: PathBuf,
        reason_file: PathBuf,
        reason: String,
        reports: Vec<ImportReport>,
    },
    /// The inbox or an entry could not be read or moved; the entry stays where it is.
    Error { name: String, message: String },
}

impl fmt::Display for InboxEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = |reports: &[ImportReport], o: Outcome| {
            reports.iter().filter(|r| r.outcome == o).count()
        };
        match self {
            InboxEvent::Waiting { name } => {
                write!(f, "waiting   {name} (imported once it stops changing)")
            }
            InboxEvent::Done {
                name,
                moved_to,
                reports,
            } => write!(
                f,
                "done      {name}: {} imported, {} already present -> {}",
                count(reports, Outcome::Imported),
                count(reports, Outcome::AlreadyPresent),
                moved_to.display()
            ),
            InboxEvent::Failed {
                name,
                moved_to,
                reason,
                ..
            } => {
                let first = reason.lines().next().unwrap_or("");
                // a result folder dropped as it is: its name already starts the reason
                let first = first
                    .strip_prefix(name.as_str())
                    .and_then(|r| r.strip_prefix(": "))
                    .unwrap_or(first);
                write!(f, "FAILED    {name}: {first} -> {}", moved_to.display())
            }
            InboxEvent::Error { name, message } => write!(f, "error     {name}: {message}"),
        }
    }
}

/// Size and newest mtime of an entry, compared between scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    bytes: u64,
    files: u64,
    newest: SystemTime,
}

#[derive(Debug)]
struct Seen {
    fp: Fingerprint,
    /// Processing failed to move it; skip until it changes.
    stuck: bool,
}

/// The inbox and what earlier scans saw.
#[derive(Debug, Default)]
pub struct Inbox {
    seen: BTreeMap<String, Seen>,
}

impl Inbox {
    pub fn new() -> Self {
        Inbox::default()
    }

    /// Look at the inbox once, import the entries that did not change since the previous scan, and move them
    /// to `done/` or `failed/`.
    pub fn scan_once(&mut self, store: &Store, opts: &InboxOptions) -> Vec<InboxEvent> {
        let mut events = Vec::new();
        let dir = &opts.dir;
        for sub in [DONE_DIR, FAILED_DIR] {
            if let Err(e) = fs::create_dir_all(dir.join(sub)) {
                events.push(InboxEvent::Error {
                    name: sub.into(),
                    message: format!("cannot create {}: {e}", dir.join(sub).display()),
                });
                return events;
            }
        }
        let entries = match candidates(dir) {
            Ok(c) => c,
            Err(e) => {
                events.push(InboxEvent::Error {
                    name: dir.display().to_string(),
                    message: format!("cannot read the inbox: {e}"),
                });
                return events;
            }
        };
        // forget entries that went away
        self.seen
            .retain(|name, _| entries.iter().any(|(n, _)| n == name));
        for (name, path) in entries {
            let fp = match fingerprint(&path) {
                Ok(Some(fp)) => fp,
                Ok(None) => continue, // an empty folder: nothing copied in yet
                Err(e) => {
                    events.push(InboxEvent::Error {
                        name,
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            match self.seen.get_mut(&name) {
                None => {
                    self.seen.insert(name.clone(), Seen { fp, stuck: false });
                    events.push(InboxEvent::Waiting { name });
                }
                Some(s) if s.fp != fp => {
                    s.fp = fp;
                    s.stuck = false;
                }
                Some(s) if s.stuck => {}
                Some(_) => {
                    let ev = process(store, dir, &name, &path);
                    match &ev {
                        InboxEvent::Error { .. } => {
                            if let Some(s) = self.seen.get_mut(&name) {
                                s.stuck = true;
                            }
                        }
                        _ => {
                            self.seen.remove(&name);
                        }
                    }
                    events.push(ev);
                }
            }
        }
        events
    }
}

/// Two scans `opts.interval` apart: enough to import whatever is complete in the inbox now.
pub fn run_once(store: &Store, opts: &InboxOptions) -> Vec<InboxEvent> {
    let mut inbox = Inbox::new();
    let mut events = inbox.scan_once(store, opts);
    if !inbox.seen.is_empty() {
        std::thread::sleep(opts.interval);
        events.extend(inbox.scan_once(store, opts));
    }
    events
}

/// Scan every `opts.interval` until `stop` is set, handing each event to `on_event`.
pub fn watch(
    store: &Store,
    opts: &InboxOptions,
    stop: &AtomicBool,
    mut on_event: impl FnMut(&InboxEvent),
) {
    let mut inbox = Inbox::new();
    let step = Duration::from_millis(100);
    while !stop.load(Ordering::Relaxed) {
        for ev in inbox.scan_once(store, opts) {
            on_event(&ev);
        }
        let mut waited = Duration::ZERO;
        while waited < opts.interval && !stop.load(Ordering::Relaxed) {
            let nap = step.min(opts.interval - waited);
            std::thread::sleep(nap);
            waited += nap;
        }
    }
}

/// The top-level entries worth looking at, sorted by name.
fn candidates(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue; // not valid UTF-8: leave it alone
        };
        if name.starts_with('.') || name == DONE_DIR || name == FAILED_DIR {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if PARTIAL_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
            continue;
        }
        let ft = e.file_type()?; // does not follow links
        let wanted = if ft.is_dir() {
            true
        } else if ft.is_file() {
            ARCHIVE_SUFFIXES.iter().any(|s| lower.ends_with(s))
        } else {
            false // links, sockets, devices
        };
        if wanted {
            out.push((name, e.path()));
        }
    }
    out.sort();
    Ok(out)
}

/// Total size, file count and newest mtime of a file or a folder tree (links are not followed).
/// `None` for a folder with no files in it yet.
fn fingerprint(path: &Path) -> io::Result<Option<Fingerprint>> {
    let mut fp = Fingerprint {
        bytes: 0,
        files: 0,
        newest: UNIX_EPOCH,
    };
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let md = fs::symlink_metadata(&p)?;
        if let Ok(m) = md.modified() {
            fp.newest = fp.newest.max(m);
        }
        if md.is_dir() {
            for e in fs::read_dir(&p)? {
                stack.push(e?.path());
            }
        } else {
            fp.bytes += md.len();
            fp.files += 1;
        }
    }
    Ok((fp.files > 0).then_some(fp))
}

/// Import one stable entry and move it out of the way.
fn process(store: &Store, dir: &Path, name: &str, path: &Path) -> InboxEvent {
    let opts = ImportOptions {
        source: "inbox".into(),
        ..ImportOptions::default()
    };
    let (reports, problems) = match import_path(store, path, &opts) {
        Ok(reports) => {
            let problems: Vec<String> = reports
                .iter()
                .filter(|r| r.outcome == Outcome::Failed)
                .map(|r| {
                    // errors about a folder usually start with its name already
                    if r.message.starts_with(&format!("{}:", r.folder)) {
                        r.message.clone()
                    } else {
                        format!("{}: {}", r.folder, r.message)
                    }
                })
                .collect();
            (reports, problems)
        }
        Err(e) => (Vec::new(), vec![format!("refused: {e}")]),
    };
    let stamp = timestamp(SystemTime::now());
    if problems.is_empty() {
        match move_into(&dir.join(DONE_DIR), &stamp, name, path, None) {
            Ok((moved_to, _)) => InboxEvent::Done {
                name: name.into(),
                moved_to,
                reports,
            },
            Err(e) => InboxEvent::Error {
                name: name.into(),
                message: format!("imported, but could not move it to {DONE_DIR}/: {e}"),
            },
        }
    } else {
        let mut reason = problems.join("\n");
        reason.push('\n');
        match move_into(&dir.join(FAILED_DIR), &stamp, name, path, Some(&reason)) {
            Ok((moved_to, reason_file)) => InboxEvent::Failed {
                name: name.into(),
                moved_to,
                reason_file: reason_file.unwrap_or_default(),
                reason: problems.join("; "),
                reports,
            },
            Err(e) => InboxEvent::Error {
                name: name.into(),
                message: format!(
                    "{} (and could not move it to {FAILED_DIR}/: {e})",
                    problems.join("; ")
                ),
            },
        }
    }
}

/// Rename `path` to `<into>/<stamp>-<name>` (or `<stamp>-<n>-<name>` when that is taken) and, with a reason,
/// write `<that>.reason.txt` first. Never replaces an existing file or folder.
fn move_into(
    into: &Path,
    stamp: &str,
    name: &str,
    path: &Path,
    reason: Option<&str>,
) -> io::Result<(PathBuf, Option<PathBuf>)> {
    for n in 1..10_000u32 {
        let base = if n == 1 {
            format!("{stamp}-{name}")
        } else {
            format!("{stamp}-{n}-{name}")
        };
        let target = into.join(&base);
        if exists(&target) {
            continue;
        }
        let reason_file = match reason {
            None => None,
            Some(text) => {
                let rf = into.join(format!("{base}.reason.txt"));
                // create_new: fails instead of overwriting
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&rf)
                {
                    Ok(mut f) => io::Write::write_all(&mut f, text.as_bytes())?,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
                Some(rf)
            }
        };
        if exists(&target) {
            continue;
        }
        fs::rename(path, &target)?;
        return Ok((target, reason_file));
    }
    Err(io::Error::other("no free name left"))
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// `YYYYMMDD-HHMMSS` in UTC.
fn timestamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_civil_dates() {
        assert_eq!(timestamp(UNIX_EPOCH), "19700101-000000");
        let t = UNIX_EPOCH + Duration::from_secs(1_795_084_373); // 2026-11-19 10:32:53 UTC
        assert_eq!(timestamp(t), "20261119-103253");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(timestamp(leap), "20000229-000000");
    }
}
