//! Compare a baseline with runs or groups stored in the database. The rule itself lives in
//! [`benchlens_rules::compare`]; this module finds the sides and their values.

use crate::store::{valid_tag_key, Store};
use crate::{Error, Result};
pub use benchlens_rules::compare::{
    compare_sides, stats, Comparison, Row, Side, Stat, Tally, ValueRow,
};
use std::collections::BTreeSet;

/// A comparison side as written (docs/api.md, REF), before it is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference<'a> {
    /// One run: a run id, a unique id prefix (4+ characters), a folder name or a label.
    Run(&'a str),
    /// `group:NAME`: every run of a group, pooled; with `@KEY=VALUE`, only its runs whose tag KEY is VALUE.
    Group {
        name: &'a str,
        tag: Option<(&'a str, &'a str)>,
    },
}

impl<'a> Reference<'a> {
    /// Read a REF. `group:NAME@KEY=VALUE` takes exactly one `@KEY=VALUE`: KEY follows the tag key rules and
    /// VALUE (matched exactly) may not hold `@`, so a group name cannot hold `@` either.
    pub fn parse(reference: &'a str) -> Result<Reference<'a>> {
        let bad = |why: &str| {
            Err(Error::BadReference(format!(
                "reference {reference:?}: {why}"
            )))
        };
        if reference.trim().is_empty() {
            return bad("empty");
        }
        let Some(rest) = reference.strip_prefix("group:") else {
            return Ok(Reference::Run(reference));
        };
        let (name, tag) = match rest.split_once('@') {
            None => (rest, None),
            Some((name, sel)) => {
                let Some((key, value)) = sel.split_once('=') else {
                    return bad("write the run selection as group:NAME@KEY=VALUE");
                };
                if !valid_tag_key(key) {
                    return bad("the tag key after @ must be 1-64 letters, digits, _ - or .");
                }
                if value.is_empty() {
                    return bad("group:NAME@KEY=VALUE needs a value");
                }
                if value.contains('@') {
                    return bad("only one @KEY=VALUE is allowed");
                }
                (name, Some((key, value)))
            }
        };
        if name.trim().is_empty() {
            return bad("group:NAME needs a name");
        }
        Ok(Reference::Group { name, tag })
    }
}

/// Find the runs of a side: `group:NAME` pools a group, `group:NAME@KEY=VALUE` the runs of that group with
/// tag KEY=VALUE, anything else is one run (id, id prefix, folder or label).
pub fn resolve(store: &Store, reference: &str) -> Result<Side> {
    let (label, run_ids) = match Reference::parse(reference)? {
        Reference::Group { name, tag: None } => {
            let run_ids = store.runs_in_group(name)?;
            if run_ids.is_empty() {
                return Err(Error::NotFound(format!(
                    "no runs in group {name:?} (see `benchlens runs`)"
                )));
            }
            (name.to_string(), run_ids)
        }
        Reference::Group {
            name,
            tag: Some((key, value)),
        } => {
            let run_ids = store.runs_in_group_tagged(name, key, value)?;
            if run_ids.is_empty() {
                let in_group = store.runs_in_group(name)?.len();
                return Err(Error::NotFound(if in_group == 0 {
                    format!("no runs in group {name:?} (see `benchlens runs`)")
                } else {
                    format!("none of the {in_group} runs of group {name:?} has tag {key}={value:?}")
                }));
            }
            (format!("{name} · {key}={value}"), run_ids)
        }
        Reference::Run(r) => {
            let id = store.find_run(r)?;
            let (label, _, _) = store.run_names(&id)?;
            (label, vec![id])
        }
    };
    Ok(Side {
        reference: reference.into(),
        label,
        run_ids,
        archs: BTreeSet::new(),
    })
}

/// A side and every value that counts for it.
pub fn load(store: &Store, reference: &str) -> Result<(Side, Vec<ValueRow>)> {
    let side = resolve(store, reference)?;
    let values = store.values(&side.run_ids)?;
    Ok((side, values))
}

/// A side and the values of one metric on it: what [`load`] gives, filtered on `metric`, without loading the
/// other metrics.
pub fn load_metric(store: &Store, reference: &str, metric: &str) -> Result<(Side, Vec<ValueRow>)> {
    let side = resolve(store, reference)?;
    let values = store.metric_values(&side.run_ids, metric)?;
    Ok((side, values))
}

/// Compare `base` with each of `others`.
pub fn compare(store: &Store, base: &str, others: &[String]) -> Result<Comparison> {
    if others.is_empty() {
        return Err(Error::NotFound(
            "nothing to compare the baseline with".into(),
        ));
    }
    let base = load(store, base)?;
    let others = others
        .iter()
        .map(|o| load(store, o))
        .collect::<Result<Vec<_>>>()?;
    Ok(compare_sides(base, others))
}
