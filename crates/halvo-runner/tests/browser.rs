//! The tests of the run's real browser (U4.1w, first cut): each starts the headless browser that `HALVO_BROWSER`
//! names, on a hosted runner alone, against fixture servers of its own on 127.0.0.1, and returns at its first line
//! off a hosted runner (`support::browser::hosted`).

mod support;

use std::time::{Duration, Instant};

use halvo_runner::LINK_END;
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{Cause, FromRunner, Outcome, RunnerCall, ToRunner};
use halvo_schema::Category;
use support::browser::{
    bearing, cond, extract_amount, from_slot, hosted, konst, navigate, own_start_us, profile_of,
    serve_with, step, Fixture,
};
use support::{slot, tag, Act, Seen, Supervisor, LONG};

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

/// A form that sends nothing: a name field, which writes the key that is pressed in it under `key`; a plan; a file
/// field, which writes the name of the chosen file under `chosen`; a button `agree`, which writes `yes` under
/// `agreed`; a button `send` that is disabled; and three rows.
const FORM: &str = r#"<!doctype html><title>form</title><form onsubmit="return false"><input data-testid="name" aria-label="Name" onkeydown="document.querySelector('[data-testid=key]').textContent = event.key"><select data-testid="plan"><option value="">-</option><option value="monthly">Monthly</option><option value="yearly">Yearly</option></select><input type="file" data-testid="file" onchange="document.querySelector('[data-testid=chosen]').textContent = this.files[0].name"><button type="button" data-testid="agree" onclick="document.querySelector('[data-testid=agreed]').textContent = 'yes'">Agree</button><button type="button" data-testid="send" disabled>Send</button></form><p data-testid="key">-</p><p data-testid="agreed">no</p><p data-testid="chosen">-</p><ul><li data-testid="row">a</li><li data-testid="row">b</li><li data-testid="row">c</li></ul>"#;

/// U4.1w part 1 (1): a run fills in a form with `fill`, `press`, `select`, `click` and `upload`, waits, and reads
/// the name back with `extract`, each step's conditions holding before and after its act; the run completes, the
/// fixture got no request but GET, and the profile is gone.
#[test]
fn a_run_fills_a_form_and_reads_it_back() {
    let Some(browser) = hosted("a_run_fills_a_form_and_reads_it_back") else {
        return;
    };
    let fixture = Fixture::start(&[("/form", FORM)]);
    let file = support::scratch("upload").join("upload.txt");
    std::fs::write(&file, "a file of the test").expect("write the file");
    let mut sup = web_agent(241, &fixture);
    sup.slots = vec![slot("file", &file.to_string_lossy())];
    let form = fixture.url("/form");
    sup.steps = vec![
        step(
            1,
            "navigate",
            None,
            &[konst("url", &form)],
            &[],
            &[cond("url_is", None, Some(&form))],
        ),
        step(
            2,
            "fill",
            Some("name"),
            &[konst("text", "Ada")],
            &[
                cond("present", Some("name"), None),
                cond("count_is", Some("row"), Some("3")),
            ],
            &[cond("value_is", Some("name"), Some("Ada"))],
        ),
        step(
            3,
            "press",
            Some("name"),
            &[konst("key", "Tab")],
            &[],
            &[cond("text_is", Some("key"), Some("Tab"))],
        ),
        step(
            4,
            "select",
            Some("plan"),
            &[konst("option", "Monthly")],
            &[],
            &[cond("value_is", Some("plan"), Some("monthly"))],
        ),
        step(
            5,
            "click",
            Some("agree"),
            &[],
            &[cond("enabled", Some("agree"), None)],
            &[cond("text_is", Some("agreed"), Some("yes"))],
        ),
        step(
            6,
            "upload",
            Some("file"),
            &[from_slot("file", "file")],
            &[],
            &[cond("text_is", Some("chosen"), Some("upload.txt"))],
        ),
        step(7, "wait", None, &[konst("ms", "100")], &[], &[]),
        step(
            8,
            "extract",
            Some("name"),
            &[konst("as", "name"), konst("parse", "text")],
            &[cond("value_is", Some("name"), Some("Ada"))],
            &[],
        ),
    ];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let ok: Vec<_> = (1..=8).map(|n| (LogEvent::StepOk, Some(n), None)).collect();
    assert_eq!(rows(&seen), ok);
    let named: Vec<(String, String)> = fixture
        .requests()
        .into_iter()
        .map(|r| (r.method, r.path))
        .collect();
    assert!(
        named.contains(&("GET".to_string(), "/form".to_string())),
        "{named:?}"
    );
    assert!(
        named.iter().all(|(method, _)| method == "GET"),
        "a request other than GET reached the fixture: {named:?}"
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// A name field, a password field that holds `SECRET`, a field that becomes a password field when it is focused,
/// and a field whose focus hands the focus on to the password field, each of which, at each input, asks its own
/// origin for `/typed` by a synchronous GET, a request that changes no state and that the rule lets through to a
/// bound origin, so that the fixture sees each act on a field.
const TYPED: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input type="password" data-testid="secret" aria-label="Password" value="halvo-runner-secret-marker" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="turns" aria-label="Code" onfocus="this.type = 'password'" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="hands" aria-label="Pin" onfocus="document.querySelector('[data-testid=secret]').focus()" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// The value of the password field of `TYPED`.
const SECRET: &str = "halvo-runner-secret-marker";

/// U4.1w part 1 (3): a step whose `post` fails after it acted, a step whose locator finds no control, a step whose
/// `pre` fails, a step stored with `credential` `no` whose locator finds a password field, one whose field becomes a
/// password field at the focus that the fill makes, and one whose field hands the focus on to the password field
/// each log one `step_failed` row with the cause `step_failed`, never `timeout`, and the run is handed back; the
/// step with the failing `pre` and the three that would reach a password field act on nothing, the one with the
/// failing `post` acted, and no frame and no line of the runner holds the password field's value (A4.3: the fresh
/// read marks the target credential before the act, and the element that holds the focus before the insert).
#[test]
fn a_post_condition_that_fails_logs_step_failed() {
    let Some(browser) = hosted("a_post_condition_that_fails_logs_step_failed") else {
        return;
    };
    let failed = [
        (LogEvent::StepOk, Some(1), None),
        (LogEvent::StepFailed, Some(2), Some(Cause::StepFailed)),
        (LogEvent::Handback, None, None),
        (LogEvent::Stopped, None, Some(Cause::StepFailed)),
    ];
    let typed = |fixture: &Fixture| {
        fixture
            .requests()
            .iter()
            .filter(|r| r.path == "/typed")
            .count()
    };
    let cases = [
        (
            251,
            step(
                2,
                "fill",
                Some("name"),
                &[konst("text", "Ada")],
                &[],
                &[cond("value_is", Some("name"), Some("Bob"))],
            ),
            true,
        ),
        (
            252,
            step(
                2,
                "fill",
                Some("missing"),
                &[konst("text", "Ada")],
                &[],
                &[],
            ),
            false,
        ),
        (
            253,
            step(
                2,
                "fill",
                Some("name"),
                &[konst("text", "Ada")],
                &[cond("present", Some("missing"), None)],
                &[],
            ),
            false,
        ),
        (
            254,
            step(2, "fill", Some("secret"), &[konst("text", "Ada")], &[], &[]),
            false,
        ),
        (
            255,
            step(2, "fill", Some("turns"), &[konst("text", "Ada")], &[], &[]),
            false,
        ),
        (
            250,
            step(2, "fill", Some("hands"), &[konst("text", "Ada")], &[], &[]),
            false,
        ),
    ];
    for (n, second, acted) in cases {
        let fixture = Fixture::start(&[("/typed-form", TYPED)]);
        let mut sup = web_agent(n, &fixture);
        sup.steps = vec![navigate(1, &fixture.url("/typed-form")), second];
        let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
        assert_eq!(seen.done(), Some(Outcome::HandedBack), "{}", seen.stderr);
        assert_eq!(rows(&seen), failed, "run {n}");
        assert_eq!(typed(&fixture) > 0, acted, "run {n}: the act on the field");
        assert!(!seen.stderr.contains(SECRET), "run {n}: {}", seen.stderr);
        for frame in &seen.frames {
            let json = frame.to_json().expect("a frame");
            assert!(!json.contains(SECRET), "run {n}: a frame holds the secret");
        }
        assert!(!profile_of(&seen, &sup.run).0.exists());
    }
}

/// The body that `SEND` posts when the field `note` holds it.
const NOTE: &str = "halvo-runner-click-marker";

/// A note field and a button that posts the note to its own origin by a synchronous request when it is clicked.
const SEND: &str = r#"<!doctype html><title>send</title><input data-testid="note" aria-label="Note"><button type="button" data-testid="send" onclick="var x = new XMLHttpRequest(); try { x.open('POST', '/sink', false); x.send(document.querySelector('[data-testid=note]').value); } catch (e) {}">Send</button>"#;

/// U4.1w part 1 (4): a run fills in a note and clicks a button that posts it; the rule in `Mode::Fail` fails the
/// POST, the run ends `refused` with the cause `origin` after the click, and no byte of the note and no request but
/// GET reaches the fixture.
#[test]
fn a_click_that_posts_reaches_no_fixture() {
    let Some(browser) = hosted("a_click_that_posts_reaches_no_fixture") else {
        return;
    };
    let fixture = Fixture::start(&[("/send", SEND)]);
    let mut sup = web_agent(243, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/send")),
        step(2, "fill", Some("note"), &[konst("text", NOTE)], &[], &[]),
        step(3, "click", Some("send"), &[], &[], &[]),
    ];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(seen.done(), Some(Outcome::Refused), "{}", seen.stderr);
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::StepOk, Some(2), None),
            (LogEvent::Stopped, None, Some(Cause::Origin)),
        ]
    );
    let got = fixture.requests();
    let named: Vec<(&str, &str)> = got
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str()))
        .collect();
    assert!(named.contains(&("GET", "/send")), "{named:?}");
    assert!(
        named.iter().all(|(method, _)| *method == "GET"),
        "a request other than GET reached the fixture: {named:?}"
    );
    assert!(
        got.iter()
            .all(|r| !r.bytes.windows(NOTE.len()).any(|w| w == NOTE.as_bytes())),
        "a byte of the note reached the fixture"
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());
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
