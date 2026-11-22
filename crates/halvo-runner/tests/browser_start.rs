//! The end of a run whose browser does not start (U4.1w, first cut, A11.1): `--browser` names an absolute path at
//! which no program stands, so the start fails at its spawn, before it starts a guard, and no browser runs.

mod support;

use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{Cause, Outcome};
use halvo_schema::Category;
use support::browser::{navigate, profile_of, serve_with};
use support::{Act, Supervisor, LONG};

/// A failed start ends the run `failed` with one row `stopped` whose step is null and whose cause is the start's,
/// `browser_start`; no handback opens, no frame `browser` is written, and the run's profile directory is gone.
#[test]
fn a_failed_start_ends_the_run_with_browser_start() {
    let missing = support::scratch("no-browser").join("no-such-browser");
    let mut sup = Supervisor::new(201, Category::UtilityBillPayment, Vec::new());
    sup.steps = vec![navigate(1, "http://127.0.0.1:9/")];
    let seen = serve_with(&sup, &missing, |_, _| Act::Answer, LONG);
    let rows: Vec<(LogEvent, Option<u32>, Option<Cause>)> = seen
        .logs()
        .iter()
        .map(|l| (l.event, l.step.map(|s| s.get()), l.cause))
        .collect();
    assert_eq!(
        rows,
        [(LogEvent::Stopped, None, Some(Cause::BrowserStart))],
        "{}",
        seen.stderr
    );
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert_eq!(seen.finish().map(|f| f.outcome), Some(Outcome::Failed));
    assert!(seen.handbacks().is_empty(), "no handback opens");
    assert!(!seen.tags().contains(&"browser"), "no frame browser");
    let (run_dir, _) = profile_of(&seen, &sup.run);
    assert!(!run_dir.exists(), "{} is left", run_dir.display());
}
