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
//!
//! Imports carry the tags of [`InboxOptions::tags`] and of a registry file ([`InboxOptions::registry`]),
//! the same way as `benchlens import --tag --registry`. The registry file is read again whenever its
//! modification time or size changes, so rows can be added while the watcher runs; a file that no longer
//! parses is reported and the previous registry is kept.
//!
//! One watcher per inbox: [`InboxLock::acquire`] takes an advisory lock on `<inbox>/.benchlens.lock` (see
//! [`LOCK_FILE`]) and holds it for the watcher's lifetime.

use crate::import::{import_path, ImportOptions, ImportReport, Outcome, Registry};
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

/// The lock file a watcher holds inside the inbox (a dotfile, so scans skip it).
pub const LOCK_FILE: &str = ".benchlens.lock";

const PARTIAL_SUFFIXES: [&str; 3] = [".part", ".tmp", ".partial"];
const ARCHIVE_SUFFIXES: [&str; 3] = [".tar", ".tar.gz", ".tgz"];

/// Where the inbox is, how often to scan it and how to tag what it imports.
#[derive(Debug, Clone)]
pub struct InboxOptions {
    pub dir: PathBuf,
    pub interval: Duration,
    /// Tags for every imported run; they win over the registry and benchlens-tags.json.
    pub tags: BTreeMap<String, String>,
    /// Registry TSV (a `run` column and one column per tag), read again when it changes.
    pub registry: Option<PathBuf>,
}

impl InboxOptions {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        InboxOptions {
            dir: dir.into(),
            interval: DEFAULT_INTERVAL,
            tags: BTreeMap::new(),
            registry: None,
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
    /// The registry file was read (at the first scan or after it changed).
    Registry { path: PathBuf, runs: usize },
    /// The inbox, an entry or the registry could not be read or moved; the entry stays where it is (a
    /// registry that cannot be read leaves the previous one in use).
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
            InboxEvent::Registry { path, runs } => {
                write!(f, "registry  {}: {runs} runs", path.display())
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

/// What the registry file looked like when it was last read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryStamp {
    /// Could not be looked at (missing, unreadable); reported once.
    Unreadable,
    /// Modification time and size.
    File(SystemTime, u64),
}

/// The registry in use and the state of the file it comes from.
#[derive(Debug)]
struct LoadedRegistry {
    path: PathBuf,
    stamp: Option<RegistryStamp>,
    registry: Option<Registry>,
}

/// The inbox and what earlier scans saw.
#[derive(Debug, Default)]
pub struct Inbox {
    seen: BTreeMap<String, Seen>,
    registry: Option<LoadedRegistry>,
}

impl Inbox {
    pub fn new() -> Self {
        Inbox::default()
    }

    /// Look at the inbox once, import the entries that did not change since the previous scan, and move them
    /// to `done/` or `failed/`.
    pub fn scan_once(&mut self, store: &Store, opts: &InboxOptions) -> Vec<InboxEvent> {
        self.scan(store, opts, None)
    }

    /// Read the registry file again when its modification time or size changed. A file that cannot be read
    /// or parsed gives one error event and the previous registry stays in use.
    fn refresh_registry(&mut self, opts: &InboxOptions, events: &mut Vec<InboxEvent>) {
        let Some(path) = &opts.registry else {
            self.registry = None;
            return;
        };
        if self.registry.as_ref().is_some_and(|l| &l.path != path) {
            self.registry = None;
        }
        let loaded = self.registry.get_or_insert_with(|| LoadedRegistry {
            path: path.clone(),
            stamp: None,
            registry: None,
        });
        let fail = |loaded: &LoadedRegistry, e: String| InboxEvent::Error {
            name: path.display().to_string(),
            message: format!(
                "cannot read the registry: {e}; {}",
                if loaded.registry.is_some() {
                    "keeping the one read before"
                } else {
                    "importing without a registry"
                }
            ),
        };
        let stamp = match fs::metadata(path).and_then(|m| Ok((m.modified()?, m.len()))) {
            Ok((t, n)) => RegistryStamp::File(t, n),
            Err(e) => {
                if loaded.stamp != Some(RegistryStamp::Unreadable) {
                    events.push(fail(loaded, e.to_string()));
                    loaded.stamp = Some(RegistryStamp::Unreadable);
                }
                return;
            }
        };
        if loaded.stamp == Some(stamp) {
            return;
        }
        // remember the stamp either way: a broken file is reported once, not on every scan
        loaded.stamp = Some(stamp);
        let parsed = fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| Registry::parse(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok(r) => {
                events.push(InboxEvent::Registry {
                    path: path.clone(),
                    runs: r.by_folder.len(),
                });
                loaded.registry = Some(r);
            }
            Err(e) => events.push(fail(loaded, e)),
        }
    }

    fn scan(
        &mut self,
        store: &Store,
        opts: &InboxOptions,
        stop: Option<&AtomicBool>,
    ) -> Vec<InboxEvent> {
        let mut events = Vec::new();
        self.refresh_registry(opts, &mut events);
        let import_opts = ImportOptions {
            source: "inbox".into(),
            tags: opts.tags.clone(),
            registry: self.registry.as_ref().and_then(|l| l.registry.clone()),
            dry_run: false,
        };
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
            if stop.is_some_and(|s| s.load(Ordering::Relaxed)) {
                break; // stopping: the rest waits for the next run
            }
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
                    let ev = process(store, &import_opts, dir, &name, &path);
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

/// Scan every `opts.interval` until `stop` is set, handing each event to `on_event`. Setting `stop` during
/// a scan lets the import in progress finish and leaves the other entries for the next run.
///
/// Take an [`InboxLock`] first so that no other watcher works on the same inbox.
pub fn watch(
    store: &Store,
    opts: &InboxOptions,
    stop: &AtomicBool,
    mut on_event: impl FnMut(&InboxEvent),
) {
    let mut inbox = Inbox::new();
    let step = Duration::from_millis(100);
    while !stop.load(Ordering::Relaxed) {
        for ev in inbox.scan(store, opts, Some(stop)) {
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

/// Who holds (or last held) an inbox lock, as written in the lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    pub pid: u32,
    pub host: String,
    /// `YYYYMMDD-HHMMSS` UTC.
    pub started: String,
}

impl LockHolder {
    /// This process, on this host, now.
    pub fn current() -> Self {
        LockHolder {
            pid: std::process::id(),
            host: hostname(),
            started: timestamp(SystemTime::now()),
        }
    }

    fn to_text(&self) -> String {
        format!(
            "pid={}\nhost={}\nstarted={}\n",
            self.pid, self.host, self.started
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim(), v.trim());
            }
        }
        Some(LockHolder {
            pid: map.get("pid")?.parse().ok()?,
            host: map.get("host").unwrap_or(&"?").to_string(),
            started: map.get("started").unwrap_or(&"?").to_string(),
        })
    }
}

impl fmt::Display for LockHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pid {} on {} (started {} UTC)",
            self.pid, self.host, self.started
        )
    }
}

/// Why an inbox lock could not be taken.
#[derive(Debug)]
pub enum LockError {
    /// Another watcher holds the lock.
    Held {
        path: PathBuf,
        holder: Option<LockHolder>,
    },
    /// The lock file could not be opened, locked or written.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held { path, holder } => {
                let inbox = path.parent().unwrap_or(path);
                match holder {
                    Some(h) => write!(f, "{} is already watched by {h}", inbox.display())?,
                    None => write!(
                        f,
                        "{} is already watched by another process",
                        inbox.display()
                    )?,
                }
                write!(
                    f,
                    "; stop that watcher first (lock file {}; remove it only if that process is gone)",
                    path.display()
                )
            }
            LockError::Io { path, source } => {
                write!(f, "cannot lock {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Io { source, .. } => Some(source),
            LockError::Held { .. } => None,
        }
    }
}

/// The exclusive right to watch one inbox, held until dropped.
///
/// It is an OS advisory lock (`flock` on unix, `LockFileEx` on Windows, through [`fs::File::try_lock`]) on
/// `<inbox>/.benchlens.lock`, so it goes away with the process however that ends. The file holds pid, host
/// and start time of the holder, for the message a second watcher prints and to spot a stale record: when
/// the OS lock is free but the file still names a holder (a watcher that was killed), the lock is taken
/// over and [`InboxLock::taken_over`] says from whom. As a guard for file systems where advisory locks do
/// nothing, a record naming another live process on this host is still refused. On drop the file is
/// emptied, not removed, so that two processes never lock two different files under the same name.
#[derive(Debug)]
pub struct InboxLock {
    file: fs::File,
    path: PathBuf,
    taken_over: Option<LockHolder>,
}

impl InboxLock {
    /// Lock the inbox `dir` (which must exist) or say who holds it.
    pub fn acquire(dir: &Path) -> Result<InboxLock, LockError> {
        use io::{Read, Seek, Write};
        let path = dir.join(LOCK_FILE);
        let io_err = |source| LockError::Io {
            path: path.clone(),
            source,
        };
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_err)?;
        let read_holder = |file: &mut fs::File| {
            let mut text = String::new();
            file.seek(io::SeekFrom::Start(0)).ok()?;
            file.read_to_string(&mut text).ok()?;
            LockHolder::parse(&text)
        };
        let os_locked = match file.try_lock() {
            Ok(()) => true,
            Err(fs::TryLockError::WouldBlock) => {
                return Err(LockError::Held {
                    holder: read_holder(&mut file),
                    path,
                })
            }
            // no advisory locks here: rely on the record alone
            Err(fs::TryLockError::Error(e)) if e.kind() == io::ErrorKind::Unsupported => false,
            Err(fs::TryLockError::Error(e)) => return Err(io_err(e)),
        };
        let me = LockHolder::current();
        let previous = read_holder(&mut file);
        if let Some(prev) = &previous {
            let same_host = prev.host == me.host;
            let alive = same_host && prev.pid != me.pid && pid_alive(prev.pid) == Some(true);
            if alive || (!os_locked && !same_host) {
                return Err(LockError::Held {
                    holder: previous,
                    path,
                });
            }
        }
        file.set_len(0).map_err(io_err)?;
        file.seek(io::SeekFrom::Start(0)).map_err(io_err)?;
        file.write_all(me.to_text().as_bytes()).map_err(io_err)?;
        file.flush().map_err(io_err)?;
        Ok(InboxLock {
            file,
            path,
            taken_over: previous,
        })
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The holder named by a stale lock file this lock replaced (a watcher that did not stop cleanly).
    pub fn taken_over(&self) -> Option<&LockHolder> {
        self.taken_over.as_ref()
    }
}

impl Drop for InboxLock {
    fn drop(&mut self) {
        // empty the record while still holding the lock; closing the file releases the lock
        let _ = self.file.set_len(0);
        let _ = self.file.unlock();
    }
}

/// Whether a process with this pid runs on this host; `None` when that cannot be told.
fn pid_alive(pid: u32) -> Option<bool> {
    if Path::new("/proc/self").exists() {
        return Some(Path::new("/proc").join(pid.to_string()).exists());
    }
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .map(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The name of this host, as well as std can tell without a system library.
fn hostname() -> String {
    let from_file = |p: &str| {
        fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|s| !s.is_empty() && cfg!(windows))
        .or_else(|| from_file("/proc/sys/kernel/hostname"))
        .or_else(|| from_file("/etc/hostname"))
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".into())
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
fn process(store: &Store, opts: &ImportOptions, dir: &Path, name: &str, path: &Path) -> InboxEvent {
    let (reports, problems) = match import_path(store, path, opts) {
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
