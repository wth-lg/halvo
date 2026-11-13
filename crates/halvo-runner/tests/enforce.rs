mod support;

use halvo_schema::executable::Outcome;
use halvo_schema::Category;

use support::{Act, FillFiles, Supervisor, LONG};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// CP-135 (5), A7.4: the handler of `start_run` stores the row 1 of the run's log, so the runner's lines count
/// from 2.
#[test]
fn the_runners_log_lines_count_from_2() {
    let files = FillFiles::new("log-from-2", 1);
    let sup = fill(140, &files);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let seqs: Vec<u32> = seen.logs().iter().map(|l| l.seq.get()).collect();
    assert_eq!(seqs, [2, 3], "the read's line, then the write's line");
}
