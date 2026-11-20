//! benchlens: import vmbench results into a SQLite database, list them and compare them.

mod export;

use anyhow::{bail, Context, Result};
use benchlens_core::catalog;
use benchlens_core::compare::{self, Comparison};
use benchlens_core::import::{self, ImportOptions, Outcome, Registry};
use benchlens_core::store::Store;
use benchlens_core::verdict::{Direction, Verdict, HIGH_CV};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "benchlens",
    version,
    about = "Import, list and compare vmbench results"
)]
struct Cli {
    /// Database file (created when missing).
    #[arg(
        long,
        global = true,
        env = "BENCHLENS_DB",
        default_value = "benchlens.db"
    )]
    db: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Import result folders, directories of result folders, or .tar / .tar.gz archives.
    Import {
        /// Folders or archives.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Tag every imported run: --tag set=poc --tag round=2 (wins over the registry and benchlens-tags.json).
        #[arg(long = "tag", value_name = "KEY=VALUE")]
        tags: Vec<String>,
        /// TSV with a `run` column (folder name) and one column per tag.
        #[arg(long)]
        registry: Option<PathBuf>,
        /// Check and show what would happen; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List the runs in the database.
    Runs {
        #[arg(long)]
        json: bool,
    },
    /// Set or remove tags on a run: benchlens tag RUN group=poc-x86-high exclude=web note=
    Tag {
        run: String,
        #[arg(required = true, value_name = "KEY=VALUE")]
        tags: Vec<String>,
    },
    /// Remove a run and its values.
    Delete { run: String },
    /// Compare a baseline with other runs or groups (group:NAME pools a group, group:NAME@KEY=VALUE part of one).
    ///
    /// A side is a run (id, unique id prefix of 4+ characters, folder or label), group:NAME (every run of a
    /// group, pooled) or group:NAME@KEY=VALUE (the runs of group NAME whose tag KEY is exactly VALUE, pooled).
    /// Example: benchlens compare group:x86-high@round=1 group:x86-high@round=2
    Compare {
        base: String,
        #[arg(required = true)]
        others: Vec<String>,
        /// Only this section: cpu, mem, disk, pg, net, web, ...
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Serve the web UI and the JSON API (docs/api.md).
    ///
    /// Settings come from the flags, then BENCHLENS_* variables (BENCHLENS_DB, _LISTEN, _INBOX,
    /// _INBOX_INTERVAL, _INBOX_REGISTRY, _TOKEN, _READ_PUBLIC, _MAX_UPLOAD_MB), then benchlens.toml, then the defaults. The token
    /// has no flag (command lines are visible to other users): set BENCHLENS_TOKEN or `token` in the file.
    Serve {
        /// Config file [default: ./benchlens.toml when present].
        #[arg(long, env = "BENCHLENS_CONFIG", value_name = "FILE")]
        config: Option<PathBuf>,
        /// Address to listen on [default: 127.0.0.1:8080]; 0.0.0.0:8080 serves other machines (set a token).
        #[arg(long, value_name = "ADDR")]
        listen: Option<String>,
        /// Also watch this inbox folder and import what is dropped into it (off when not given).
        #[arg(long, value_name = "DIR")]
        inbox: Option<PathBuf>,
        /// Seconds between two inbox scans [default: 5].
        #[arg(long, value_name = "SECS")]
        inbox_interval: Option<u64>,
        /// Registry TSV for inbox imports (a `run` column and one column per tag); read again when it changes.
        #[arg(long, value_name = "FILE")]
        inbox_registry: Option<PathBuf>,
        /// Tag every run the inbox imports: --inbox-tag set=poc (wins over the registry).
        #[arg(long = "inbox-tag", value_name = "KEY=VALUE")]
        inbox_tags: Vec<String>,
        /// false: reading the API needs the token too [default: true].
        #[arg(long, value_name = "BOOL", action = clap::ArgAction::Set,
              value_parser = clap::builder::BoolishValueParser::new())]
        read_public: Option<bool>,
        /// Largest upload accepted by POST /api/runs, in MiB [default: 256].
        #[arg(long, value_name = "MB")]
        max_upload_mb: Option<u64>,
    },
    /// Watch an inbox folder: import result folders and archives dropped into it, then move them to done/
    /// or failed/ (with a .reason.txt).
    Watch {
        /// The inbox folder.
        #[arg(long, env = "BENCHLENS_INBOX", default_value = "inbox")]
        inbox: PathBuf,
        /// Seconds between two scans; an entry is imported once it is unchanged for one interval.
        #[arg(long, value_name = "SECS", default_value_t = 5)]
        interval: u64,
        /// Scan twice, one interval apart, then exit.
        #[arg(long)]
        once: bool,
        /// Registry TSV (a `run` column and one column per tag); read again when it changes.
        #[arg(long, env = "BENCHLENS_INBOX_REGISTRY", value_name = "FILE")]
        registry: Option<PathBuf>,
        /// Tag every imported run: --tag set=poc --tag round=2 (wins over the registry and benchlens-tags.json).
        #[arg(long = "tag", value_name = "KEY=VALUE")]
        tags: Vec<String>,
    },
    /// Export a comparison as one self-contained HTML file, or the values of sides as JSON / CSV.
    Export {
        #[command(subcommand)]
        cmd: export::ExportCmd,
    },
    /// Metric tables of vmbench versions.
    Catalog {
        #[command(subcommand)]
        cmd: CatalogCmd,
    },
}

#[derive(Subcommand)]
enum CatalogCmd {
    /// List the vmbench versions that have a metric table.
    List,
    /// Add the table of a newer vmbench: vmbench run --metrics > m.txt; benchlens catalog add 0.3.0 m.txt
    Add { version: String, file: PathBuf },
}

fn parse_tags(list: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for t in list {
        let (k, v) = benchlens_core::store::parse_tag(t)?;
        out.insert(k, v);
    }
    Ok(out)
}

/// Set `stop` on Ctrl-C (and SIGTERM on unix) so `watch` ends after the import in progress; a second signal
/// exits at once. Returns once the handlers are in place.
fn stop_on_signal(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
    use std::sync::atomic::Ordering;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<()>>();
    std::thread::Builder::new()
        .name("benchlens-signals".into())
        .spawn(move || {
            rt.block_on(async move {
                #[cfg(unix)]
                let signals = {
                    use tokio::signal::unix::{signal, SignalKind};
                    signal(SignalKind::interrupt())
                        .and_then(|i| Ok((i, signal(SignalKind::terminate())?)))
                };
                #[cfg(unix)]
                let (mut int, mut term) = match signals {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                #[cfg(not(unix))]
                let _ = ready_tx.send(Ok(()));
                loop {
                    #[cfg(unix)]
                    tokio::select! {
                        _ = int.recv() => {}
                        _ = term.recv() => {}
                    }
                    #[cfg(not(unix))]
                    if tokio::signal::ctrl_c().await.is_err() {
                        return;
                    }
                    if stop.swap(true, Ordering::Relaxed) {
                        eprintln!("stopping now");
                        std::process::exit(130);
                    }
                    eprintln!("stopping after the import in progress (Ctrl-C again to stop now)");
                }
            })
        })?;
    ready_rx
        .recv()
        .context("signal handler thread ended")?
        .context("installing signal handlers")
}

fn main() -> Result<()> {
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    match cli.cmd {
        Cmd::Import {
            paths,
            tags,
            registry,
            dry_run,
            json,
        } => {
            let store =
                Store::open(&cli.db).with_context(|| format!("opening {}", cli.db.display()))?;
            let registry = match registry {
                Some(p) => Some(Registry::parse(
                    &fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?,
                )?),
                None => None,
            };
            let opts = ImportOptions {
                source: "cli".into(),
                tags: parse_tags(&tags)?,
                registry,
                dry_run,
            };
            let mut reports = Vec::new();
            let mut refused = 0;
            for p in &paths {
                match import::import_path(&store, p, &opts) {
                    Ok(r) => reports.extend(r),
                    Err(e) => {
                        refused += 1;
                        eprintln!("refused {}: {e}", p.display());
                    }
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&reports)?);
            } else {
                for r in &reports {
                    let what = match r.outcome {
                        Outcome::Imported => "imported",
                        Outcome::AlreadyPresent => "present ",
                        Outcome::WouldImport => "would import",
                        Outcome::Failed => "FAILED  ",
                    };
                    println!(
                        "{what}  {:<16}  {}  {}",
                        r.run_id.as_deref().unwrap_or("-"),
                        r.folder,
                        r.message
                    );
                }
                let n = |o: Outcome| reports.iter().filter(|r| r.outcome == o).count();
                println!(
                    "{} imported, {} already present, {} would import, {} failed, {} refused{}",
                    n(Outcome::Imported),
                    n(Outcome::AlreadyPresent),
                    n(Outcome::WouldImport),
                    n(Outcome::Failed),
                    refused,
                    if dry_run {
                        " (dry run: nothing written)"
                    } else {
                        ""
                    }
                );
            }
            if refused > 0 || reports.iter().any(|r| r.outcome == Outcome::Failed) {
                std::process::exit(1);
            }
        }
        Cmd::Runs { json } => {
            let store = Store::open(&cli.db)?;
            let runs = store.list_runs()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&runs)?);
            } else if runs.is_empty() {
                println!("no runs yet: benchlens import <result folder>");
            } else {
                println!(
                    "{:<16}  {:<28}  {:<8}  {:>4}  {:<7}  GROUP / TAGS",
                    "RUN", "LABEL", "ARCH", "vCPU", "VMBENCH"
                );
                for r in runs {
                    let tags: Vec<String> = r
                        .tags
                        .iter()
                        .filter(|(k, _)| *k != "group")
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    let mut flags = String::new();
                    if !r.catalog_known {
                        flags.push_str(" [no metric table]");
                    }
                    if !r.failed_stages.is_empty() {
                        flags.push_str(&format!(" [failed: {}]", r.failed_stages.join(",")));
                    }
                    println!(
                        "{:<16}  {:<28}  {:<8}  {:>4}  {:<7}  {}{}{}",
                        r.run_id,
                        r.label,
                        r.arch,
                        r.nproc.map(|n| n.to_string()).unwrap_or_default(),
                        r.vmbench_version,
                        r.group,
                        if tags.is_empty() {
                            String::new()
                        } else {
                            format!("  {}", tags.join(" "))
                        },
                        flags
                    );
                }
            }
        }
        Cmd::Tag { run, tags } => {
            let store = Store::open(&cli.db)?;
            let id = store.find_run(&run)?;
            store.set_tags(&id, &parse_tags(&tags)?)?;
            println!("tags set on {id}");
        }
        Cmd::Delete { run } => {
            let store = Store::open(&cli.db)?;
            let id = store.find_run(&run)?;
            store.delete_run(&id)?;
            println!("deleted {id}");
        }
        Cmd::Compare {
            base,
            others,
            section,
            json,
        } => {
            let store = Store::open(&cli.db)?;
            let mut c = compare::compare(&store, &base, &others)?;
            if let Some(s) = &section {
                c.rows.retain(|r| &r.section == s);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                print_comparison(&c);
            }
        }
        Cmd::Serve {
            config,
            listen,
            inbox,
            inbox_interval,
            inbox_registry,
            inbox_tags,
            read_public,
            max_upload_mb,
        } => {
            use benchlens_server::config::{config_path, Layer, Settings};
            // --db is global with a default; only a value typed on the command line counts as a flag here,
            // so BENCHLENS_DB and the file keep their place in the order.
            let db_typed = matches
                .subcommand_matches("serve")
                .and_then(|m| m.value_source("db"))
                .or_else(|| matches.value_source("db"))
                == Some(clap::parser::ValueSource::CommandLine);
            let flags = Layer {
                db: db_typed.then(|| cli.db.clone()),
                listen,
                inbox,
                inbox_interval,
                inbox_registry,
                inbox_tags: (!inbox_tags.is_empty())
                    .then(|| parse_tags(&inbox_tags))
                    .transpose()?,
                token: None,
                read_public,
                max_upload_mb,
            };
            let env = Layer::from_env(|k| std::env::var(k).ok())?;
            let (path, explicit) = config_path(config);
            let file = Layer::from_file(&path, explicit)?;
            let settings = Settings::resolve(&flags, &env, &file)?;
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(benchlens_server::serve(settings))?;
        }
        Cmd::Watch {
            inbox,
            interval,
            once,
            registry,
            tags,
        } => {
            use benchlens_core::inbox::{self, InboxOptions};
            let store =
                Store::open(&cli.db).with_context(|| format!("opening {}", cli.db.display()))?;
            let opts = InboxOptions {
                interval: std::time::Duration::from_secs(interval.max(1)),
                tags: parse_tags(&tags)?,
                registry,
                ..InboxOptions::new(inbox)
            };
            let lock = benchlens_server::prepare_inbox(&opts)?;
            if once {
                for ev in inbox::run_once(&store, &opts) {
                    println!("{ev}");
                }
            } else {
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                stop_on_signal(stop.clone())?;
                eprintln!(
                    "watching {} every {}s (Ctrl-C to stop)",
                    opts.dir.display(),
                    opts.interval.as_secs()
                );
                inbox::watch(&store, &opts, &stop, |ev| println!("{ev}"));
                eprintln!("stopped watching {}", opts.dir.display());
            }
            drop(lock);
        }
        Cmd::Export { cmd } => export::run(&cli.db, cmd)?,
        Cmd::Catalog { cmd } => {
            let store = Store::open(&cli.db)?;
            match cmd {
                CatalogCmd::List => {
                    for (v, builtin, n) in store.catalogs()? {
                        println!(
                            "{v:<10} {n:>4} metrics  {}",
                            if builtin { "built in" } else { "added" }
                        );
                    }
                }
                CatalogCmd::Add { version, file } => {
                    let cat = catalog::parse(&version, &fs::read_to_string(&file)?);
                    if cat.metrics.is_empty() {
                        bail!("{}: no `id|unit|direction|tool|label|description` lines (output of `vmbench run --metrics`)", file.display());
                    }
                    let n = store.put_catalog(&cat, false)?;
                    println!("added {n} metrics for vmbench {}", cat.version);
                }
            }
        }
    }
    Ok(())
}

fn fmt_num(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        let s = format!("{:.0}", v);
        let (sign, digits) = s.split_at(if s.starts_with('-') { 1 } else { 0 });
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        format!("{sign}{out}")
    } else if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}

/// Widest side label shown in a column head of the compare table.
const MAX_HEAD: usize = 32;

/// Column heads for the two value columns: the side labels, padded to at least 12 characters and cut at
/// [`MAX_HEAD`]. Labels that are still the same after cutting (or were the same to begin with) become
/// "baseline" and "other", so the columns never look alike.
fn column_heads(base: &str, other: &str) -> (String, String) {
    let head = |l: &str| {
        let n = l.chars().count();
        let cut: String = if n > MAX_HEAD {
            l.chars().take(MAX_HEAD - 1).chain(['…']).collect()
        } else {
            l.to_string()
        };
        format!("{cut:>12}")
    };
    let (b, o) = (head(base), head(other));
    if b.trim() == o.trim() {
        (format!("{:>12}", "baseline"), format!("{:>12}", "other"))
    } else {
        (b, o)
    }
}

fn print_comparison(c: &Comparison) {
    println!(
        "baseline: {} ({} run{})",
        c.base.label,
        c.base.run_ids.len(),
        if c.base.run_ids.len() == 1 { "" } else { "s" }
    );
    for (i, side) in c.others.iter().enumerate() {
        let t = &c.tallies[i];
        println!(
            "\n== {} vs {}: better {}, worse {}, within noise {}",
            side.label, c.base.label, t.better, t.worse, t.not_clear
        );
        let secs: Vec<String> = t
            .by_section
            .iter()
            .map(|(s, [b, w, n])| format!("{s} {b}/{w}/{n}"))
            .collect();
        println!(
            "   by section (better/worse/within noise): {}",
            secs.join("  ")
        );
        let (head_base, head_side) = column_heads(&c.base.label, &side.label);
        let (wb, ws) = (head_base.chars().count(), head_side.chars().count());
        println!(
            "   {:<44} {head_base:>wb$} {head_side:>ws$} {:>8}  verdict",
            "metric", "diff"
        );
        for r in c.rows.iter().filter(|r| r.other == i) {
            let verdict = if r.cross_arch {
                "not counted (arch)"
            } else {
                match r.verdict {
                    Verdict::Better => "better",
                    Verdict::Worse => "worse",
                    Verdict::NotClear => "within noise",
                }
            };
            let noisy =
                if r.cv_base.unwrap_or(0.0) >= HIGH_CV || r.cv_value.unwrap_or(0.0) >= HIGH_CV {
                    " !"
                } else {
                    ""
                };
            let once = if r.n_base < 2 || r.n_value < 2 {
                " *"
            } else {
                ""
            };
            let dir = if r.direction == Direction::Lower {
                "↓"
            } else {
                "↑"
            };
            println!(
                "   {:<44} {:>wb$} {:>ws$} {:>8}  {verdict}{noisy}{once}",
                format!("{} {dir} ({})", r.name, r.unit)
                    .chars()
                    .take(44)
                    .collect::<String>(),
                fmt_num(r.base),
                fmt_num(r.value),
                r.diff
                    .map(|d| format!("{d:+.1}%"))
                    .unwrap_or_else(|| "-".into())
            );
        }
    }
    println!("\n↑ higher is better, ↓ lower is better; * measured once (noise unknown); ! CV of {HIGH_CV:.0}% or more");
    for w in &c.warnings {
        println!("note: {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::column_heads;

    #[test]
    fn column_heads_stay_distinguishable() {
        // the labels of group subset sides share a long prefix: both must show in full
        let (b, o) = column_heads("x86-high · round=1", "x86-high · round=2");
        assert_eq!(
            (b.as_str(), o.as_str()),
            ("x86-high · round=1", "x86-high · round=2")
        );
        // short labels keep the 12-character column
        let (b, o) = column_heads("x86", "arm");
        assert_eq!((b.chars().count(), o.chars().count()), (12, 12));
        // cut at the limit, still different
        let long = "g".repeat(40);
        let (b, o) = column_heads(&format!("{long}1"), &format!("{long}2"));
        assert_ne!(b, o);
        assert!(b.trim() == "baseline" && o.trim() == "other");
        let (b, _) = column_heads(&long, "x");
        assert_eq!(b.chars().count(), 32);
        assert!(b.ends_with('…'));
    }
}
