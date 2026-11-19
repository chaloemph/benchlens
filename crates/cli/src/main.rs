//! benchlens: import vmbench results into a SQLite database, list them and compare them.

use anyhow::{bail, Context, Result};
use benchlens_core::catalog;
use benchlens_core::compare::{self, Comparison};
use benchlens_core::import::{self, ImportOptions, Outcome, Registry};
use benchlens_core::store::Store;
use benchlens_core::verdict::{Direction, Verdict, HIGH_CV};
use clap::{Parser, Subcommand};
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
    Serve {
        /// Address to listen on; use 0.0.0.0:8080 to serve other machines.
        #[arg(long, env = "BENCHLENS_LISTEN", default_value = "127.0.0.1:8080")]
        listen: std::net::SocketAddr,
        /// Also watch this inbox folder and import what is dropped into it (off when not given).
        #[arg(long, env = "BENCHLENS_INBOX", value_name = "DIR")]
        inbox: Option<PathBuf>,
        /// Seconds between two inbox scans.
        #[arg(long, value_name = "SECS", default_value_t = 5)]
        inbox_interval: u64,
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
        let (k, v) = t
            .split_once('=')
            .with_context(|| format!("tag {t:?}: write KEY=VALUE"))?;
        out.insert(k.trim().to_string(), v.trim().to_string());
    }
    Ok(out)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
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
            listen,
            inbox,
            inbox_interval,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(benchlens_server::serve(benchlens_server::ServeConfig {
                db: cli.db.clone(),
                listen,
                inbox,
                inbox_interval: std::time::Duration::from_secs(inbox_interval.max(1)),
            }))?;
        }
        Cmd::Watch {
            inbox,
            interval,
            once,
        } => {
            let store =
                Store::open(&cli.db).with_context(|| format!("opening {}", cli.db.display()))?;
            let opts = benchlens_core::inbox::InboxOptions {
                dir: inbox,
                interval: std::time::Duration::from_secs(interval.max(1)),
            };
            fs::create_dir_all(&opts.dir)
                .with_context(|| format!("creating inbox {}", opts.dir.display()))?;
            if once {
                for ev in benchlens_core::inbox::run_once(&store, &opts) {
                    println!("{ev}");
                }
            } else {
                eprintln!(
                    "watching {} every {}s (Ctrl-C to stop)",
                    opts.dir.display(),
                    opts.interval.as_secs()
                );
                let stop = std::sync::atomic::AtomicBool::new(false);
                benchlens_core::inbox::watch(&store, &opts, &stop, |ev| println!("{ev}"));
            }
        }
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
        println!(
            "   {:<44} {:>12} {:>12} {:>8}  verdict",
            "metric",
            c.base.label.chars().take(12).collect::<String>(),
            side.label.chars().take(12).collect::<String>(),
            "diff"
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
                "   {:<44} {:>12} {:>12} {:>8}  {verdict}{noisy}{once}",
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
