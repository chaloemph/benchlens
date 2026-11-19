//! Compare a baseline with one or more runs or groups, metric by metric, with vmbench's rule
//! ([`crate::verdict::judge`]).
//!
//! A side is either one run or a group: the values of every run in the group are pooled as one sample
//! (median and CV of all of them), so repeated test rounds count as replicates.

use crate::store::{Store, ValueRow};
use crate::verdict::{self, Direction, Verdict};
use crate::{Error, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One side of a comparison.
#[derive(Debug, Clone, Serialize)]
pub struct Side {
    /// What the user asked for (`group:NAME` or a run reference).
    pub reference: String,
    pub label: String,
    pub run_ids: Vec<String>,
    pub archs: BTreeSet<String>,
}

/// Median and spread of one metric on one side.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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
    pub diff: Option<f64>,
    pub noise: f64,
    pub verdict: Verdict,
    /// sysbench across architectures: shown, not counted (it depends on the compiler).
    pub cross_arch: bool,
}

/// Counts of verdicts for one compared side.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Tally {
    pub better: usize,
    pub worse: usize,
    pub not_clear: usize,
    pub by_section: BTreeMap<String, [usize; 3]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub base: Side,
    pub others: Vec<Side>,
    pub rows: Vec<Row>,
    pub tallies: Vec<Tally>,
    pub warnings: Vec<String>,
}

/// `group:NAME` pools a group; anything else is one run (id, id prefix, folder or label).
pub fn resolve(store: &Store, reference: &str) -> Result<Side> {
    if let Some(group) = reference.strip_prefix("group:") {
        let run_ids = store.runs_in_group(group)?;
        if run_ids.is_empty() {
            return Err(Error::NotFound(format!(
                "no runs in group {group:?} (see `benchlens runs`)"
            )));
        }
        return Ok(Side {
            reference: reference.into(),
            label: group.into(),
            run_ids,
            archs: BTreeSet::new(),
        });
    }
    let id = store.find_run(reference)?;
    let (label, _, _) = store.run_names(&id)?;
    Ok(Side {
        reference: reference.into(),
        label,
        run_ids: vec![id],
        archs: BTreeSet::new(),
    })
}

/// Median, CV, min and max per metric of a side. Values without a known direction (no metric table for that
/// vmbench version) are left out and counted in the second result.
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

/// Compare `base` with each of `others`.
pub fn compare(store: &Store, base: &str, others: &[String]) -> Result<Comparison> {
    if others.is_empty() {
        return Err(Error::NotFound(
            "nothing to compare the baseline with".into(),
        ));
    }
    let mut warnings = Vec::new();
    let mut base_side = resolve(store, base)?;
    let base_stats = load(store, &mut base_side, &mut warnings)?;
    let mut sides = Vec::new();
    let mut rows = Vec::new();
    let mut tallies = Vec::new();
    for (i, reference) in others.iter().enumerate() {
        let mut side = resolve(store, reference)?;
        let st = load(store, &mut side, &mut warnings)?;
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
    Ok(Comparison {
        base: base_side,
        others: sides,
        rows,
        tallies,
        warnings,
    })
}

fn load(
    store: &Store,
    side: &mut Side,
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<String, Stat>> {
    let values = store.values(&side.run_ids)?;
    side.archs = values.iter().map(|v| v.arch.clone()).collect();
    let (st, unknown) = stats(&values);
    if unknown > 0 {
        warnings.push(format!(
            "{}: {unknown} values from a vmbench version without a metric table were left out (`benchlens catalog add`)",
            side.label
        ));
    }
    Ok(st)
}
