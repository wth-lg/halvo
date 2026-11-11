pub fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

pub fn join_pieces(pieces: &[String]) -> String {
    fold(&pieces.join(" "))
}

pub fn distance(a: &[char], b: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorRate {
    pub distance: usize,
    pub known_chars: usize,
}

impl ErrorRate {
    pub fn percent(&self) -> f64 {
        100.0 * self.distance as f64 / self.known_chars as f64
    }
}

pub fn error_rate(engine_text: &str, known_text: &str) -> Option<ErrorRate> {
    let known: Vec<char> = fold(known_text).chars().collect();
    if known.is_empty() {
        return None;
    }
    let engine: Vec<char> = fold(engine_text).chars().collect();
    Some(ErrorRate {
        distance: distance(&engine, &known),
        known_chars: known.len(),
    })
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[middle])
    } else {
        Some((sorted[middle - 1] + sorted[middle]) / 2.0)
    }
}
