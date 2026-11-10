//! Order statistics of a small sample.

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub min: f64,
    pub median: f64,
    pub p90: f64,
    pub max: f64,
}

pub fn summarize(sample: &[f64]) -> Option<Summary> {
    if sample.is_empty() {
        return None;
    }
    let mut sorted = sample.to_vec();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let median = if count % 2 == 1 {
        sorted[count / 2]
    } else {
        (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
    };
    // Nearest rank: the smallest value with at least 90% of the sample at or below it.
    let p90 = sorted[(count * 9).div_ceil(10) - 1];
    Some(Summary {
        min: sorted[0],
        median,
        p90,
        max: sorted[count - 1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_values() {
        let sample: Vec<f64> = (1..=30).rev().map(f64::from).collect();
        assert_eq!(
            summarize(&sample),
            Some(Summary {
                min: 1.0,
                median: 15.5,
                p90: 27.0,
                max: 30.0
            })
        );
    }

    #[test]
    fn one_value_and_none() {
        assert_eq!(
            summarize(&[7.0]),
            Some(Summary {
                min: 7.0,
                median: 7.0,
                p90: 7.0,
                max: 7.0
            })
        );
        assert_eq!(summarize(&[]), None);
    }
}
