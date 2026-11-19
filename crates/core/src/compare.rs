//! Compare a baseline with runs or groups stored in the database. The rule itself lives in
//! [`benchlens_rules::compare`]; this module finds the sides and their values.

use crate::store::Store;
use crate::{Error, Result};
pub use benchlens_rules::compare::{
    compare_sides, stats, Comparison, Row, Side, Stat, Tally, ValueRow,
};
use std::collections::BTreeSet;

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

/// A side and every value that counts for it.
pub fn load(store: &Store, reference: &str) -> Result<(Side, Vec<ValueRow>)> {
    let side = resolve(store, reference)?;
    let values = store.values(&side.run_ids)?;
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
