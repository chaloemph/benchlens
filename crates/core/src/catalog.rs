//! The vmbench metric table, one per vmbench version: the output of `vmbench run --metrics`,
//! one line per id: `id|unit|direction|tool|label|description`.
//!
//! The tables of known versions are built into benchlens (`crates/core/catalog/`); a table for a newer
//! vmbench can be added to a database with `benchlens catalog add` without a new benchlens release.

use crate::verdict::Direction;
use serde::Serialize;
use std::collections::BTreeMap;

const EMBEDDED: &[(&str, &str)] = &[
    ("0.2.1", include_str!("../catalog/vmbench-0.2.1.txt")),
    ("0.2.2", include_str!("../catalog/vmbench-0.2.2.txt")),
    ("0.2.3", include_str!("../catalog/vmbench-0.2.3.txt")),
];

/// One metric of the vmbench table.
#[derive(Debug, Clone, Serialize)]
pub struct Metric {
    pub id: String,
    pub unit: String,
    pub direction: Direction,
    pub tool: String,
    pub label: String,
    pub description: String,
}

impl Metric {
    /// Name that reads the same on every machine size (see [`display_name`]).
    pub fn name(&self) -> String {
        display_name(&self.id, &self.label)
    }
    /// First part of the id: cpu, mem, disk, pg, net, web, ...
    pub fn section(&self) -> &str {
        section(&self.id)
    }
}

/// The metric table of one vmbench version.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub version: String,
    pub metrics: BTreeMap<String, Metric>,
}

/// "v0.2.3" and "0.2.3" are the same version.
pub fn normalize_version(v: &str) -> String {
    v.trim().trim_start_matches('v').to_string()
}

/// Parse the `vmbench run --metrics` text. Lines that do not have six fields are skipped.
pub fn parse(version: &str, text: &str) -> Catalog {
    let mut metrics = BTreeMap::new();
    for line in text.lines() {
        let p: Vec<&str> = line.splitn(6, '|').collect();
        if p.len() < 6 || p[0].trim().is_empty() {
            continue;
        }
        metrics.insert(
            p[0].to_string(),
            Metric {
                id: p[0].to_string(),
                unit: p[1].to_string(),
                direction: Direction::parse(p[2]),
                tool: p[3].to_string(),
                label: p[4].to_string(),
                description: p[5].to_string(),
            },
        );
    }
    Catalog {
        version: normalize_version(version),
        metrics,
    }
}

/// The built-in table of a vmbench version, if benchlens knows it.
pub fn embedded(version: &str) -> Option<Catalog> {
    let v = normalize_version(version);
    EMBEDDED
        .iter()
        .find(|(ver, _)| *ver == v)
        .map(|(ver, text)| parse(ver, text))
}

/// Versions with a built-in table.
pub fn embedded_versions() -> Vec<&'static str> {
    EMBEDDED.iter().map(|(v, _)| *v).collect()
}

/// First part of an id.
pub fn section(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

/// Label template -> a name that is the same on every machine: thread counts (`{t}`) and per-run values (`{x}`)
/// go, and `.1t` / `.max` ids get " · 1 thread" / " · all vCPU".
pub fn display_name(id: &str, label: &str) -> String {
    let mut s = label.to_string();
    for pat in [
        " {t}t",
        "{t}t",
        " (N={x})",
        " (-c {x})",
        " ({x}s)",
        " -t {x}MB",
        "{x}",
        "{t}",
    ] {
        s = s.replace(pat, "");
    }
    let s = s.trim().to_string();
    if id.ends_with(".1t") {
        format!("{s} · 1 thread")
    } else if id.ends_with(".max") {
        format!("{s} · all vCPU")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tables_parse() {
        for v in embedded_versions() {
            let c = embedded(v).unwrap();
            assert!(c.metrics.len() > 90, "{v}: {} metrics", c.metrics.len());
            assert_eq!(
                c.metrics["pg.pgbench.tpcb.tps"].direction,
                Direction::Higher
            );
            assert_eq!(c.metrics["net.iperf3.udp.loss"].direction, Direction::Lower);
        }
        assert!(embedded("v0.2.3").is_some());
        assert!(embedded("9.9.9").is_none());
    }

    #[test]
    fn names() {
        assert_eq!(
            display_name("cpu.sysbench.prime.max", "sysbench cpu {t}t"),
            "sysbench cpu · all vCPU"
        );
        assert_eq!(
            display_name("cpu.7z.rating.max", "7z rating {t}t"),
            "7z rating · all vCPU"
        );
        assert_eq!(
            display_name("mem.stream.copy.max", "stream copy (N={x})"),
            "stream copy · all vCPU"
        );
        assert_eq!(
            display_name("mem.lmbench.lat-mem-rd.1t", "lat_mem_rd -t {x}MB"),
            "lat_mem_rd · 1 thread"
        );
        assert_eq!(
            display_name("pg.pgbench.tpcb.tps", "pgbench tpcb-like tps (-c {x})"),
            "pgbench tpcb-like tps"
        );
    }
}
