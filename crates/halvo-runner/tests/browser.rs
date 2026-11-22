//! The tests of the run's real browser (U4.1w, first cut): each starts the headless browser that `HALVO_BROWSER`
//! names, on a hosted runner alone, against fixture servers of its own on 127.0.0.1, and returns at its first line
//! off a hosted runner (`support::browser::hosted`). The one test of U9.10 gives no `--browser`, so that the runner
//! starts the browser that it finds itself, and returns at its first line off a hosted macOS runner
//! (`support::browser::hosted_mac`).

mod support;

use std::time::{Duration, Instant};

use halvo_runner::LINK_END;
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{Cause, FromRunner, Outcome, RunnerCall, ToRunner};
use halvo_schema::Category;
use support::browser::{
    bearing, extract_amount, hosted, hosted_mac, navigate, own_start_us, profile_of, say,
    serve_finding, serve_with, Fixture,
};
use support::{tag, Act, Seen, Supervisor, FINDS_ITS_BROWSER, LONG};

/// A page whose one control with the test id `amount` reads `12.34`.
const READ: &str = r#"<!doctype html><title>read</title><p data-testid="amount">12.34</p>"#;

/// The body of the state-changing request of `POST`.
const MARKER: &str = "halvo-runner-post-marker";

/// A page that sends a POST with the body `MARKER` to its own origin while it loads, by a synchronous request, so
/// that the browser has answered it before the page is loaded.
const POST: &str = r#"<!doctype html><title>post</title><script>var x = new XMLHttpRequest(); try { x.open("POST", "/sink", false); x.send("halvo-runner-post-marker"); } catch (e) {}</script><p>sent</p>"#;

fn browser_reply() -> ToRunner {
    ToRunner::parse(br#"{"t":"browser_reply"}"#).expect("a browser_reply")
}

/// The supervisor's act: `browser_reply` for the frame `browser`, the store's answer for every other frame.
fn reply_to_browser(frame: &FromRunner, _: &[FromRunner]) -> Act {
    match frame {
        FromRunner::Browser(_) => Act::With(browser_reply()),
        _ => Act::Answer,
    }
}

/// The run's log rows by event, step and cause.
fn rows(seen: &Seen) -> Vec<(LogEvent, Option<u32>, Option<Cause>)> {
    seen.logs()
        .iter()
        .map(|l| (l.event, l.step.map(|s| s.get()), l.cause))
        .collect()
}

fn web_agent(n: u8, fixture: &Fixture) -> Supervisor {
    let mut sup = Supervisor::new(n, Category::UtilityBillPayment, Vec::new());
    sup.origins = vec![fixture.origin()];
    sup
}

/// U4.1w (1): the runner starts the browser at the run's first web step, writes the frame `browser` once and reads
/// its `browser_reply` before any request reaches the fixture, then opens the page of its bound origin and reads
/// the control's amount; the run completes and the profile is gone.
#[test]
fn a_run_reads_a_page_after_the_frame_browser() {
    let Some(browser) = hosted("a_run_reads_a_page_after_the_frame_browser") else {
        return;
    };
    let fixture = Fixture::start(&[("/read", READ)]);
    let mut sup = web_agent(211, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/read")),
        extract_amount(2, "amount"),
    ];
    let mut at_frame = None;
    let seen = serve_with(
        &sup,
        &browser,
        |frame, _| match frame {
            FromRunner::Browser(_) => {
                let before = fixture.requests().len();
                std::thread::sleep(Duration::from_millis(500));
                at_frame = Some((before, fixture.requests().len()));
                Act::With(browser_reply())
            }
            _ => Act::Answer,
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::StepOk, Some(2), None)
        ]
    );
    assert_eq!(
        at_frame,
        Some((0, 0)),
        "no request reached the fixture before the reply to the frame browser"
    );
    let tags: Vec<&str> = seen.frames.iter().map(tag).collect();
    assert_eq!(tags.iter().filter(|t| **t == "browser").count(), 1);
    let browser_at = tags.iter().position(|t| *t == "browser");
    assert!(
        browser_at > tags.iter().position(|t| *t == "progress"),
        "{tags:?}"
    );
    let got: Vec<(String, String)> = fixture
        .requests()
        .into_iter()
        .map(|r| (r.method, r.path))
        .collect();
    assert!(
        got.contains(&("GET".to_string(), "/read".to_string())),
        "{got:?}"
    );
    let (run_dir, _) = profile_of(&seen, &sup.run);
    assert!(!run_dir.exists(), "{} is left", run_dir.display());
    assert!(
        seen.stderr.contains("run browser=stopped removed=yes"),
        "{}",
        seen.stderr
    );
}

/// U9.10 (5): a runner given no `--browser` on a hosted macOS runner starts the browser that it finds at the fixed
/// places of a Mac, opens the page of its bound origin and reads the control's amount; the run completes and the
/// profile is gone. The test names which of the two places holds a file.
#[test]
fn a_run_given_no_browser_starts_the_one_it_finds() {
    if !hosted_mac("a_run_given_no_browser_starts_the_one_it_finds") {
        return;
    }
    for place in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ] {
        let file = std::path::Path::new(place).is_file();
        say(
            FINDS_ITS_BROWSER,
            &format!("{place}: {}", if file { "a file" } else { "none" }),
        );
    }
    let fixture = Fixture::start(&[("/read", READ)]);
    let mut sup = web_agent(241, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/read")),
        extract_amount(2, "amount"),
    ];
    let seen = serve_finding(&sup, FINDS_ITS_BROWSER, reply_to_browser, LONG);
    assert!(
        !seen.line.iter().any(|a| a == "--browser"),
        "{:?}",
        seen.line
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::StepOk, Some(2), None)
        ]
    );
    let got: Vec<(String, String)> = fixture
        .requests()
        .into_iter()
        .map(|r| (r.method, r.path))
        .collect();
    assert!(
        got.contains(&("GET".to_string(), "/read".to_string())),
        "{got:?}"
    );
    let (run_dir, _) = profile_of(&seen, &sup.run);
    assert!(!run_dir.exists(), "{} is left", run_dir.display());
}

/// U4.1w (3): with the bindings' origins armed in `Mode::Fail`, a load of an origin outside them fails and ends the
/// run `refused` with the cause `origin`, and that origin's fixture gets no connection; a page of a bound origin
/// that sends a POST while it loads has it failed, no byte of it reaches the fixture, and the run ends the same way.
#[test]
fn a_state_changing_request_reaches_no_fixture() {
    let Some(browser) = hosted("a_state_changing_request_reaches_no_fixture") else {
        return;
    };
    let inside = Fixture::start(&[("/post", POST)]);
    let outside = Fixture::start(&[("/read", READ)]);
    let refused = [(LogEvent::Stopped, None, Some(Cause::Origin))];

    let mut sup = web_agent(221, &inside);
    sup.steps = vec![navigate(1, &outside.url("/read"))];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(seen.done(), Some(Outcome::Refused), "{}", seen.stderr);
    assert_eq!(rows(&seen), refused);
    assert!(seen.handbacks().is_empty());
    assert_eq!(
        outside.requests().len(),
        0,
        "a connection reached the origin outside the bindings"
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());

    let mut sup = web_agent(222, &inside);
    sup.steps = vec![navigate(1, &inside.url("/post"))];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(seen.done(), Some(Outcome::Refused), "{}", seen.stderr);
    assert_eq!(rows(&seen), refused);
    let got = inside.requests();
    let named: Vec<(&str, &str)> = got
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str()))
        .collect();
    assert!(named.contains(&("GET", "/post")), "{named:?}");
    assert!(
        named.iter().all(|(method, _)| *method == "GET"),
        "a request other than GET reached the fixture: {named:?}"
    );
    assert!(
        got.iter().all(|r| !r
            .bytes
            .windows(MARKER.len())
            .any(|w| w == MARKER.as_bytes())),
        "a byte of the POST's body reached the fixture"
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// Whether `frame` stores the log row `step_ok` of step `n`.
fn step_ok_of(frame: &FromRunner, n: u32) -> bool {
    matches!(frame, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Log(b)
        if b.records.iter().any(|r| r.event == LogEvent::StepOk && r.step.map(|s| s.get()) == Some(n))))
}

/// U4.1w (4): when the supervisor closes the link while the run's page is open, halvo-runner and every process that
/// bears the run's profile are gone within `LINK_END` of the close, and the profile's directory is removed.
#[test]
fn the_browser_is_gone_within_link_end_of_the_close() {
    let Some(browser) = hosted("the_browser_is_gone_within_link_end_of_the_close") else {
        return;
    };
    let since = own_start_us();
    let fixture = Fixture::start(&[("/read", READ)]);
    let mut sup = web_agent(231, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/read")),
        extract_amount(2, "amount"),
    ];
    let seen = serve_with(
        &sup,
        &browser,
        |frame, earlier| match frame {
            FromRunner::Store(_) if step_ok_of(frame, 1) => Act::Close,
            _ => reply_to_browser(frame, earlier),
        },
        LONG,
    );
    let close_to_end = seen
        .close_to_end
        .unwrap_or_else(|| panic!("the runner did not end after the close: {}", seen.stderr));
    assert!(
        close_to_end <= LINK_END,
        "halvo-runner ended {close_to_end:?} after the close"
    );
    assert_eq!(seen.code(), Some(1), "{}", seen.stderr);
    let (run_dir, profile) = profile_of(&seen, &sup.run);
    let until = Instant::now() + LINK_END.saturating_sub(close_to_end);
    let left = loop {
        let left = bearing(&profile, since);
        if left.is_empty() || Instant::now() >= until {
            break left;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let pids: Vec<u32> = left.iter().map(|p| p.pid).collect();
    assert!(
        pids.is_empty(),
        "processes that bear the run's profile at LINK_END: {pids:?}"
    );
    assert!(!run_dir.exists(), "{} is left", run_dir.display());
}

/// U4.1w (5), the row `win_single_argument_inside_quotes` of `terminator_table` (crates/halvo-web/src/checks.rs):
/// a browser started with the one argument `--app=x --single-argument`, which the system writes in quotes, before
/// the profile's switch takes that profile directory: the directory exists and holds an entry.
#[cfg(windows)]
#[test]
fn the_browser_takes_the_profile_after_a_quoted_single_argument() {
    let Some(browser) = hosted("the_browser_takes_the_profile_after_a_quoted_single_argument")
    else {
        return;
    };
    let profile = support::scratch("single-argument").join("profile");
    let mut child = std::process::Command::new(&browser)
        .arg("--app=x --single-argument")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--headless=new",
            "--no-first-run",
            "--no-default-browser-check",
        ])
        .arg("about:blank")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the browser starts");
    let until = Instant::now() + Duration::from_secs(30);
    let took = loop {
        let took = std::fs::read_dir(&profile).is_ok_and(|mut d| d.next().is_some());
        if took || Instant::now() >= until {
            break took;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &child.id().to_string()])
        .output();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        took,
        "the browser did not take {} after a quoted --single-argument",
        profile.display()
    );
}
