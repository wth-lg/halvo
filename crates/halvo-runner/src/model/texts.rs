//! The fixed texts of a model request (contracts A9.5), each a copy byte for byte of the driver trial's file under
//! tools/r3-trials/drive, which one test holds against the trial's file: a change of either side is a revision of
//! the trial's texts, and the test reads it.

/// The boundary text, the file crates/halvo-runner/prompts/boundary.txt, which stands at the head of the `system`
/// of every model request (A9.5).
pub(crate) const BOUNDARY: &str = include_str!("../../prompts/boundary.txt");

#[cfg(test)]
mod tests {
    use super::BOUNDARY;

    /// Each copy of the runner and the trial's file that it copies, by their paths from the workspace's root.
    const COPIES: [(&str, &str); 1] = [(
        "crates/halvo-runner/prompts/boundary.txt",
        "tools/r3-trials/drive/prompts/boundary.txt",
    )];

    #[test]
    fn u72b_the_texts_of_a_request_are_the_trials_byte_for_byte() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |path: &str| std::fs::read(root.join(path)).expect("a text of the tree");
        for (copy, trial) in COPIES {
            let (ours, theirs) = (read(copy), read(trial));
            println!(
                "copy: {copy}: {} bytes; the trial's {trial}: {} bytes",
                ours.len(),
                theirs.len()
            );
            assert!(ours == theirs, "{copy} differs from {trial}");
        }
        assert_eq!(BOUNDARY.as_bytes(), read(COPIES[0].0));
    }
}
