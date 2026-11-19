//! Compare a baseline with one or more sides, metric by metric, with vmbench's rule ([`crate::verdict::judge`]).
//!
//! A side is one run or a group of runs: the values of every run on a side are pooled as one sample (median
//! and CV of all of them), so repeated test rounds count as replicates. This module only sees values; finding
//! them (database, files) is the caller's job.

use crate::verdict::{self, Direction, Verdict};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One raw value with what is needed to judge it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueRow {
    pub run_id: String,
    pub id: String,
    pub name: String,
    pub section: String,
    pub unit: String,
    /// None when the run's vmbench version has no metric table: such values are not judged.
    pub direction: Option<Direction>,
    pub tool: String,
    pub arch: String,
    pub round: i64,
    pub value: f64,
}

/// One side of a comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Side {
    /// What was asked for (`group:NAME` or a run reference).
    pub reference: String,
    pub label: String,
    pub run_ids: Vec<String>,
    /// Architectures of the values on this side (filled by [`compare_sides`]).
    #[serde(default)]
    pub archs: BTreeSet<String>,
}

/// Median and spread of one metric on one side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stat {
    pub id: String,
    pub name: String,
    pub section: String,
    pub unit: String,
    pub direction: Direction,
    pub tool: String,
    pub n: usize,
    pub median: f64,
    pub cv: Option<f64>,
    pub min: f64,
    pub max: f64,
}

/// One metric of one compared side against the baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    /// Index into [`Comparison::others`].
    pub other: usize,
    pub id: String,
    pub name: String,
    pub section: String,
    pub unit: String,
    pub direction: Direction,
    pub base: f64,
    pub value: f64,
    pub cv_base: Option<f64>,
    pub cv_value: Option<f64>,
    pub n_base: usize,
    pub n_value: usize,
    /// (value - base) / |base| in %; None when the baseline is 0.
    pub diff: Option<f64>,
    /// The threshold judge() used: max(1 %, CV of either side).
    pub noise: f64,
    pub verdict: Verdict,
    /// sysbench across architectures: shown, not counted (it depends on the compiler).
    pub cross_arch: bool,
}

/// Counts of verdicts for one compared side (sysbench across architectures not counted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tally {
    pub better: usize,
    pub worse: usize,
    pub not_clear: usize,
    /// section -> [better, worse, not clear]
    pub by_section: BTreeMap<String, [usize; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub base: Side,
    pub others: Vec<Side>,
    pub rows: Vec<Row>,
    /// One per entry of `others`, same order.
    pub tallies: Vec<Tally>,
    pub warnings: Vec<String>,
}

/// Median, CV, min and max per metric. Values without a known direction are left out and counted in the
/// second result.
pub fn stats(values: &[ValueRow]) -> (BTreeMap<String, Stat>, usize) {
    let mut acc: BTreeMap<String, (Vec<f64>, &ValueRow)> = BTreeMap::new();
    let mut unknown = 0;
    for v in values {
        if v.direction.is_none() {
            unknown += 1;
            continue;
        }
        acc.entry(v.id.clone())
            .or_insert_with(|| (Vec::new(), v))
            .0
            .push(v.value);
    }
    let out = acc
        .into_iter()
        .map(|(id, (xs, v))| {
            let stat = Stat {
                id: id.clone(),
                name: v.name.clone(),
                section: v.section.clone(),
                unit: v.unit.clone(),
                direction: v.direction.unwrap_or(Direction::None),
                tool: v.tool.clone(),
                n: xs.len(),
                median: verdict::median(&xs).unwrap_or(f64::NAN),
                cv: verdict::cv(&xs),
                min: xs.iter().copied().fold(f64::INFINITY, f64::min),
                max: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            };
            (id, stat)
        })
        .collect();
    (out, unknown)
}

/// Judge every side in `others` against `base`. Each side comes with all of its values.
pub fn compare_sides(
    base: (Side, Vec<ValueRow>),
    others: Vec<(Side, Vec<ValueRow>)>,
) -> Comparison {
    let mut warnings = Vec::new();
    let (mut base_side, base_values) = base;
    let base_stats = side_stats(&mut base_side, &base_values, &mut warnings);
    let mut sides = Vec::new();
    let mut rows = Vec::new();
    let mut tallies = Vec::new();
    for (i, (mut side, values)) in others.into_iter().enumerate() {
        let st = side_stats(&mut side, &values, &mut warnings);
        let cross = side.archs != base_side.archs;
        let mut tally = Tally::default();
        for (id, b) in &base_stats {
            let Some(o) = st.get(id) else { continue };
            let (diff, v) = verdict::judge(b.median, o.median, b.direction, b.cv, o.cv);
            let Some(v) = v else { continue };
            let cross_arch = cross && b.tool.contains("sysbench");
            if !cross_arch {
                let slot = tally.by_section.entry(b.section.clone()).or_default();
                match v {
                    Verdict::Better => {
                        tally.better += 1;
                        slot[0] += 1;
                    }
                    Verdict::Worse => {
                        tally.worse += 1;
                        slot[1] += 1;
                    }
                    Verdict::NotClear => {
                        tally.not_clear += 1;
                        slot[2] += 1;
                    }
                }
            }
            rows.push(Row {
                other: i,
                id: id.clone(),
                name: b.name.clone(),
                section: b.section.clone(),
                unit: b.unit.clone(),
                direction: b.direction,
                base: b.median,
                value: o.median,
                cv_base: b.cv,
                cv_value: o.cv,
                n_base: b.n,
                n_value: o.n,
                diff,
                noise: verdict::noise(b.cv, o.cv),
                verdict: v,
                cross_arch,
            });
        }
        if cross && base_stats.keys().any(|k| k.starts_with("cpu.sysbench.")) {
            warnings.push(format!(
                "{} and {} are different architectures: sysbench cpu is shown but not counted (it depends on the compiler)",
                base_side.label, side.label
            ));
        }
        tallies.push(tally);
        sides.push(side);
    }
    Comparison {
        base: base_side,
        others: sides,
        rows,
        tallies,
        warnings,
    }
}

fn side_stats(
    side: &mut Side,
    values: &[ValueRow],
    warnings: &mut Vec<String>,
) -> BTreeMap<String, Stat> {
    side.archs = values.iter().map(|v| v.arch.clone()).collect();
    let (st, unknown) = stats(values);
    if unknown > 0 {
        warnings.push(format!(
            "{}: {unknown} values from a vmbench version without a metric table were left out (`benchlens catalog add`)",
            side.label
        ));
    }
    st
}
