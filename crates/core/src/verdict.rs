//! Median, CV and the verdict rule. This must stay identical to `judge()` in vmbench `report.py` (v0.2.3):
//! `tests/golden.rs` checks every metric of several fixture pairs against verdicts produced by vmbench itself.

use serde::Serialize;

/// Smaller differences (in %) are never called a win (report.py `MIN_DIFF`).
pub const MIN_DIFF: f64 = 1.0;
/// A coefficient of variation (in %) from which a value is flagged as noisy (report.py `HIGH_CV`).
pub const HIGH_CV: f64 = 5.0;

/// Which way is better for a metric (the `direction` column of the vmbench metric table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Higher,
    Lower,
    /// A context value (CPU steal, a flag) that is shown but never judged.
    None,
}

impl Direction {
    pub fn parse(s: &str) -> Direction {
        match s {
            "higher" => Direction::Higher,
            "lower" => Direction::Lower,
            _ => Direction::None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Higher => "higher",
            Direction::Lower => "lower",
            Direction::None => "none",
        }
    }
}

/// The outcome of comparing one metric of a machine with the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The compared machine is better than the baseline.
    Better,
    /// The baseline is better.
    Worse,
    /// The difference is within noise.
    NotClear,
}

impl Verdict {
    /// report.py's encoding: +1 other better, -1 baseline better, 0 not clear.
    pub fn as_i8(self) -> i8 {
        match self {
            Verdict::Better => 1,
            Verdict::Worse => -1,
            Verdict::NotClear => 0,
        }
    }
}

/// Median as Python's `statistics.median` (mean of the two middle values for an even count).
pub fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    Some(if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    })
}

/// Coefficient of variation in %: population standard deviation over |mean| (`statistics.pstdev`).
/// None for fewer than two values or a zero mean, as in report.py.
pub fn cv(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    if mean == 0.0 {
        return None;
    }
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    Some(var.sqrt() / mean.abs() * 100.0)
}

/// (difference in %, verdict) of `other` against `base`. A `Direction::None` metric is never judged.
pub fn judge(
    base: f64,
    other: f64,
    direction: Direction,
    cv_base: Option<f64>,
    cv_other: Option<f64>,
) -> (Option<f64>, Option<Verdict>) {
    if direction == Direction::None {
        return (None, None);
    }
    let diff = if base != 0.0 {
        Some((other - base) / base.abs() * 100.0)
    } else {
        None
    };
    if base == other {
        return (diff, Some(Verdict::NotClear));
    }
    if let Some(d) = diff {
        let noise = MIN_DIFF
            .max(cv_base.unwrap_or(0.0))
            .max(cv_other.unwrap_or(0.0));
        if d.abs() <= noise {
            return (diff, Some(Verdict::NotClear));
        }
    }
    let better = (other > base) == (direction == Direction::Higher);
    (
        diff,
        Some(if better {
            Verdict::Better
        } else {
            Verdict::Worse
        }),
    )
}

/// The noise threshold judge() used, for display.
pub fn noise(cv_base: Option<f64>, cv_other: Option<f64>) -> f64 {
    MIN_DIFF
        .max(cv_base.unwrap_or(0.0))
        .max(cv_other.unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_even_and_odd() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn cv_matches_pstdev() {
        // pstdev([2,4,4,4,5,5,7,9]) = 2, mean 5 -> 40 %
        let c = cv(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert!((c - 40.0).abs() < 1e-12);
        assert_eq!(cv(&[1.0]), None);
        assert_eq!(cv(&[-1.0, 1.0]), None);
    }

    #[test]
    fn judge_rules() {
        let h = Direction::Higher;
        assert_eq!(
            judge(100.0, 100.5, h, None, None).1,
            Some(Verdict::NotClear)
        ); // under 1 %
        assert_eq!(
            judge(100.0, 104.0, h, Some(5.0), None).1,
            Some(Verdict::NotClear)
        ); // within CV
        assert_eq!(judge(100.0, 110.0, h, None, None).1, Some(Verdict::Better));
        assert_eq!(
            judge(100.0, 110.0, Direction::Lower, None, None).1,
            Some(Verdict::Worse)
        );
        assert_eq!(judge(1.0, 2.0, Direction::None, None, None), (None, None));
        assert_eq!(
            judge(0.0, 2.0, h, None, None),
            (None, Some(Verdict::Better))
        ); // no % from a zero baseline
        assert_eq!(
            judge(0.0, 0.0, h, None, None),
            (None, Some(Verdict::NotClear))
        );
    }
}
