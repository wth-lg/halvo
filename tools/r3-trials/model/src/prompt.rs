/// The marker lines around page and message text (contracts.md A9.5).
pub const OPEN: &str = "<<<DATA";
pub const CLOSE: &str = "DATA>>>";

/// The longest start of `snapshot` that ends at a line end and that `count` gives at most
/// `limit` tokens (A12.2), without the line feed that ends it. The whole text is kept when it
/// fits; no line is kept when the first one does not fit.
pub fn cut_snapshot(snapshot: &str, limit: usize, count: impl Fn(&str) -> usize) -> &str {
    let whole = snapshot.strip_suffix('\n').unwrap_or(snapshot);
    if count(whole) <= limit {
        return whole;
    }
    let ends: Vec<usize> = whole.match_indices('\n').map(|(at, _)| at).collect();
    let (mut fits, mut over) = (0usize, ends.len() + 1);
    while over - fits > 1 {
        let mid = (fits + over) / 2;
        if count(&whole[..ends[mid - 1]]) <= limit {
            fits = mid;
        } else {
            over = mid;
        }
    }
    if fits == 0 {
        ""
    } else {
        &whole[..ends[fits - 1]]
    }
}

/// The user turn of a request: the prompt, then the snapshot between the marker lines.
pub fn user_text(prompt: &str, snapshot: Option<&str>) -> String {
    match snapshot {
        Some(data) => format!("{prompt}\n{OPEN}\n{data}\n{CLOSE}"),
        None => prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn a_snapshot_that_fits_is_kept_whole() {
        assert_eq!(cut_snapshot("a b\nc d\n", 4, words), "a b\nc d");
        assert_eq!(cut_snapshot("a b\nc d", 4, words), "a b\nc d");
        assert_eq!(cut_snapshot("", 4, words), "");
    }

    #[test]
    fn a_long_snapshot_is_cut_at_a_line_end() {
        let text = "a b\nc d\ne f\ng h\n";
        assert_eq!(cut_snapshot(text, 7, words), "a b\nc d\ne f");
        assert_eq!(cut_snapshot(text, 6, words), "a b\nc d\ne f");
        assert_eq!(cut_snapshot(text, 5, words), "a b\nc d");
        assert_eq!(cut_snapshot(text, 2, words), "a b");
        assert_eq!(cut_snapshot(text, 1, words), "");
        assert_eq!(cut_snapshot(text, 0, words), "");
    }

    #[test]
    fn the_cut_takes_the_longest_start_that_fits() {
        let text: String = (1..=50).map(|i| format!("w{i} x\n")).collect();
        for limit in 0..=101 {
            let cut = cut_snapshot(&text, limit, words);
            assert!(words(cut) <= limit, "{limit}");
            let lines = cut.split('\n').filter(|l| !l.is_empty()).count();
            assert_eq!(lines, (limit / 2).min(50), "{limit}");
        }
    }

    #[test]
    fn the_snapshot_goes_after_the_prompt_between_the_markers() {
        assert_eq!(
            user_text("Pay the bill.", Some("e1 button \"Pay\"")),
            "Pay the bill.\n<<<DATA\ne1 button \"Pay\"\nDATA>>>"
        );
        assert_eq!(user_text("Pay the bill.", None), "Pay the bill.");
        assert_eq!(user_text("Pay.", Some("")), "Pay.\n<<<DATA\n\nDATA>>>");
    }
}
