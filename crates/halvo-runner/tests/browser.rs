//! The tests of the run's real browser (U4.1w, first cut): each starts the headless browser that `HALVO_BROWSER`
//! names, on a hosted runner alone, against fixture servers of its own on 127.0.0.1, and returns at its first line
//! off a hosted runner (`support::browser::hosted`). The one test of U9.10 gives no `--browser`, so that the runner
//! starts the browser that it finds itself, and returns at its first line off a hosted macOS runner
//! (`support::browser::hosted_mac`).

mod support;

use std::time::{Duration, Instant};

use halvo_runner::LINK_END;
use halvo_schema::executable::names::Answer;
use halvo_schema::executable::names::{HandbackReason, LogEvent};
use halvo_schema::executable::EffectState;
use halvo_schema::executable::{Cause, FromRunner, Outcome, RunnerCall, ToRunner};
use halvo_schema::Category;
use support::browser::{
    bearing, cond, entries_below, extract_amount, fixture_sign_in, framed_cond, framed_step,
    from_slot, handback, hosted, hosted_headed, hosted_mac, hosted_on_mac, konst, navigate,
    own_start_us, pay_click, print_entries, print_outside_the_profile, profile_of, say,
    serve_finding, serve_in, serve_with, step, Fixture, Forwarder, Made, SEED_1,
};
use support::connect::decision;
use support::{slot, tag, Act, Seen, Supervisor, FINDS_ITS_BROWSER, LONG};

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
/// A connection on which no request came is no request, since a browser may open a connection and send nothing on it
/// (run 45083944176, its windows job): the check of the method reads the connections that carried a request, and the
/// check of the POST's body reads every connection.
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
        named
            .iter()
            .filter(|(method, _)| !method.is_empty())
            .all(|(method, _)| *method == "GET"),
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
/// bound origin, so that the fixture sees each act on a field; and a field with no input of its own whose focus
/// hands the focus on into the password field of `INNER`, which an object element shows.
const TYPED: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input type="password" data-testid="secret" aria-label="Password" value="halvo-runner-secret-marker" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="turns" aria-label="Code" onfocus="this.type = 'password'" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="hands" aria-label="Pin" onfocus="document.querySelector('[data-testid=secret]').focus()" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="into" aria-label="Note" onfocus="document.querySelector('object').contentDocument.querySelector('input').focus()"><object data="/inner" type="text/html"></object>"#;

/// A page served as XHTML, whose browser names its elements in lower case, with one password field that holds
/// `XHTML_SECRET` and asks `/typed` at each input.
const XHTML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>xhtml</title></head><body><input type="password" data-testid="pw" aria-label="Password" value="halvo-runner-xhtml-marker" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"/></body></html>"#;

/// The value of the password field of `XHTML`.
const XHTML_SECRET: &str = "halvo-runner-xhtml-marker";

/// The document of the object element of `TYPED`: a password field that asks `/typed` at each input.
const INNER: &str = r#"<!doctype html><title>inner</title><input type="password" aria-label="Password" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// `TYPED` with its name field alone.
const NAMED: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// The value of the password field of `TYPED`.
const SECRET: &str = "halvo-runner-secret-marker";

/// `TYPED` with its name field and its field that becomes a password field when it is focused, and no password field.
const TURNS: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="turns" aria-label="Code" onfocus="this.type = 'password'" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// `TYPED` with its name field and a field whose focus makes a password field and hands the focus on to it, and no
/// password field before that focus.
const HANDS: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="hands" aria-label="Pin" onfocus="var p = document.createElement('input'); p.type = 'password'; p.setAttribute('aria-label', 'Password'); p.oninput = function () { var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {} }; document.body.appendChild(p); p.focus();" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// `TYPED` with its name field, its field with no input of its own whose focus hands the focus on into the password
/// field of `INNER`, and the object element that shows `INNER`, and no password field of its own.
const INTO: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input data-testid="into" aria-label="Note" onfocus="document.querySelector('object').contentDocument.querySelector('input').focus()"><object data="/inner" type="text/html"></object>"#;

/// `TYPED` with its name field and a code field of the type number whose `autocomplete` is `one-time-code`, which the
/// page's tree gives the role `spinbutton`.
const CODE: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input type="number" data-testid="code" aria-label="Code" autocomplete="one-time-code" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">"#;

/// `TYPED` with its name field and a password field that the page hides (`display: none`).
const HIDDEN: &str = r#"<!doctype html><title>typed</title><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}"><input type="password" data-testid="secret" aria-label="Password" style="display: none">"#;

/// A page whose script makes a password field in the document of a frame of its own and moves it into itself, so that
/// the page's world does not know the field as one of its own, and a button that asks its own origin for `/typed`
/// when it is clicked.
const FOREIGN: &str = r#"<!doctype html><title>typed</title><iframe></iframe><button type="button" data-testid="go" onclick="var x = new XMLHttpRequest(); try { x.open('GET', '/typed', false); x.send(); } catch (e) {}">Go</button><script>var made = document.querySelector('iframe').contentDocument.createElement('input'); made.type = 'password'; made.setAttribute('data-testid', 'moved'); made.setAttribute('aria-label', 'Secret'); document.body.appendChild(made);</script>"#;

/// U4.1w part 1 (3), U5.1: a step whose `post` fails after it acted, a step whose locator finds no control and a
/// step whose `pre` fails, on a page with no password field, each log one `step_failed` row with the cause
/// `step_failed`, never `timeout`, and the run is handed back. A step stored with `credential` `no` whose locator
/// finds a password field meets the runner's reading of the page before the act; a fill of a field that becomes a
/// password field at the focus that the fill makes, and a fill of a field whose focus hands the focus on to a
/// password field, each on a page with no password field before that focus, are refused because the runner types
/// only into an element that holds the focus of the four kinds that it reads whole, and meet the runner's reading
/// after the refused act: each opens a handback of the reason `sign_in`, and the supervisor's `declined` ends the run
/// `declined`. A fill of a field whose focus hands the focus on into an object's document, on a page with no password
/// field of its own, is refused the same way and is handed back with `step_failed`: the reading enters the frames of
/// the page's tree alone and not an object's document, so it does not see the object's password field (run
/// 44998838561 read this end in both jobs). A fill of the name field on a page with a code field of the type number
/// meets the reading's sweep of every field, which opens a handback of the reason `second_factor`; and a fill of the
/// password field of a page served as XHTML meets the reading before the act, which knows the field by the browser's
/// own interface and not by the lower-case tag name that the browser gives it there: a handback of the reason
/// `sign_in`, declined; and a click whose `pre` reads the value of a password field that the page's script made in a
/// frame's document and moved into itself meets the reading, which reads it in the executor's own world as what it is,
/// a password field by its type (r3 U5.13): a handback of the reason `sign_in`, declined, no click and no value
/// read. A fill of the name field beside a password field that the page hides acts and opens no handback: the
/// reading counts a field only where the browser says it is shown (the coordinator's ruling of log line 14244). No step that would reach a password field types a byte, the one with the failing `post`
/// acted, and no frame and no line of the runner holds a password field's value.
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
    let declined = [
        (LogEvent::StepOk, Some(1), None),
        (LogEvent::Handback, None, None),
        (LogEvent::Stopped, None, Some(Cause::Declined)),
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
        (
            249,
            step(2, "fill", Some("into"), &[konst("text", "Ada")], &[], &[]),
            false,
        ),
        (
            247,
            step(2, "fill", Some("name"), &[konst("text", "Ada")], &[], &[]),
            false,
        ),
        (
            244,
            step(2, "fill", Some("name"), &[konst("text", "Ada")], &[], &[]),
            true,
        ),
        (
            246,
            step(
                2,
                "click",
                Some("go"),
                &[],
                &[cond("value_is", Some("moved"), Some("x"))],
                &[],
            ),
            false,
        ),
    ];
    for (n, second, acted) in cases {
        let page = match n {
            254 => TYPED,
            255 => TURNS,
            250 => HANDS,
            249 => INTO,
            247 => CODE,
            246 => FOREIGN,
            244 => HIDDEN,
            _ => NAMED,
        };
        let fixture = Fixture::start(&[("/typed-form", page), ("/inner", INNER)]);
        let mut sup = web_agent(n, &fixture);
        sup.steps = vec![navigate(1, &fixture.url("/typed-form")), second];
        let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
        if n == 244 {
            assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
            assert_eq!(
                rows(&seen),
                [
                    (LogEvent::StepOk, Some(1), None),
                    (LogEvent::StepOk, Some(2), None)
                ],
                "run {n}"
            );
            assert!(seen.handbacks().is_empty(), "run {n}");
        } else if n == 246 {
            assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
            assert_eq!(rows(&seen), declined, "run {n}");
            assert_eq!(seen.handbacks(), [HandbackReason::SignIn], "run {n}");
        } else if n == 247 {
            assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
            assert_eq!(rows(&seen), declined, "run {n}");
            assert_eq!(seen.handbacks(), [HandbackReason::SecondFactor], "run {n}");
        } else if matches!(n, 250 | 254 | 255) {
            assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
            assert_eq!(rows(&seen), declined, "run {n}");
            assert_eq!(seen.handbacks(), [HandbackReason::SignIn], "run {n}");
        } else {
            assert_eq!(seen.done(), Some(Outcome::HandedBack), "{}", seen.stderr);
            assert_eq!(rows(&seen), failed, "run {n}");
        }
        assert_eq!(typed(&fixture) > 0, acted, "run {n}: the act on the field");
        assert!(!seen.stderr.contains(SECRET), "run {n}: {}", seen.stderr);
        for frame in &seen.frames {
            let json = frame.to_json().expect("a frame");
            assert!(!json.contains(SECRET), "run {n}: a frame holds the secret");
        }
        assert!(!profile_of(&seen, &sup.run).0.exists());
    }

    // Case 248: a password field of a page served as XHTML, known by the browser's own interface and not by the
    // lower-case name that the browser gives it there: the reading before the act opens a sign_in handback, which the
    // supervisor declines; typed into by nothing, its value read by nothing.
    let fixture = Fixture::start(&[("/form.xhtml", XHTML)]);
    let mut sup = web_agent(248, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/form.xhtml")),
        step(2, "fill", Some("pw"), &[konst("text", "Ada")], &[], &[]),
    ];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
    assert_eq!(rows(&seen), declined, "run 248");
    assert_eq!(seen.handbacks(), [HandbackReason::SignIn], "run 248");
    assert_eq!(typed(&fixture), 0, "run 248: the act on the field");
    assert!(
        !seen.stderr.contains(XHTML_SECRET),
        "run 248: {}",
        seen.stderr
    );
    for frame in &seen.frames {
        let json = frame.to_json().expect("a frame");
        assert!(
            !json.contains(XHTML_SECRET),
            "run 248: a frame holds the secret"
        );
    }
    assert!(!profile_of(&seen, &sup.run).0.exists());
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

/// A sign-in page: a name field and a password field. At a load with no cookie `seen` its script sets that cookie;
/// at a load that finds it, its script sends a POST to `/signed-in` 2 s later, as a person's sign-in does, and takes
/// the password field out of the page. The handback's opening keeps the cookies when it clears the origin's stores
/// and loads the page again by GET, so the second load is the one that signs in.
const SIGN_IN: &str = r#"<!doctype html><title>sign in</title><form onsubmit="return false"><input data-testid="user" aria-label="Email"><input type="password" data-testid="password" aria-label="Password"></form><p data-testid="state">out</p><script>if (document.cookie.indexOf('seen=1') < 0) { document.cookie = 'seen=1; path=/'; } else { setTimeout(function () { var x = new XMLHttpRequest(); try { x.open('POST', '/signed-in', false); x.send('in'); } catch (e) {} var p = document.querySelector('[data-testid=password]'); p.parentNode.removeChild(p); document.querySelector('[data-testid=state]').textContent = 'in'; }, 2000); }</script>"#;

/// U5.1, A6.4, on the hosted macos job alone: a headed run opens the sign-in page, and its `handback` step reads a
/// credential field there and opens a handback of the reason `sign_in` that names the fixture's origin, after it has
/// loaded the page again by GET;
/// while the handback is open the page's own POST reaches the fixture and its script takes the password field out;
/// the test answers `done` 8 s after the request, the runner finds no credential field, sends `bind` with the
/// fixture's origin, and after `bound=yes` the run goes on to a page of the bound origin and ends `completed`. After
/// the browser's stop it prints what stands outside the profile for the browser's bundle id (names, sizes and whether
/// each changed in the run; the coordinator's ruling of log line 14152), and asserts nothing of it.
#[test]
fn a_headed_run_hands_back_at_a_sign_in_and_goes_on_after_the_bind() {
    let Some(browser) =
        hosted_headed("a_headed_run_hands_back_at_a_sign_in_and_goes_on_after_the_bind")
    else {
        return;
    };
    let started = std::time::SystemTime::now();
    let fixture = Fixture::start(&[("/sign-in", SIGN_IN), ("/read", READ)]);
    let mut sup = web_agent(245, &fixture);
    sup.headed = true;
    sup.steps = vec![
        navigate(1, &fixture.url("/sign-in")),
        handback(2),
        navigate(3, &fixture.url("/read")),
        extract_amount(4, "amount"),
    ];
    let done = ToRunner::parse(
        br#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#,
    )
    .expect("a handback_reply");
    let bound = ToRunner::parse(br#"{"t":"bind_reply","bound":"yes"}"#).expect("a bind_reply");
    let seen = serve_with(
        &sup,
        &browser,
        |frame, _| match frame {
            FromRunner::Browser(_) => Act::With(browser_reply()),
            FromRunner::Handback(_) => {
                Act::After(Duration::from_secs(8), Box::new(Act::With(done.clone())))
            }
            FromRunner::Bind(_) => Act::With(bound.clone()),
            _ => Act::Answer,
        },
        Duration::from_secs(120),
    );
    print_outside_the_profile(
        "a_headed_run_hands_back_at_a_sign_in_and_goes_on_after_the_bind",
        started,
        std::time::SystemTime::now(),
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(seen.handbacks(), [HandbackReason::SignIn]);
    let asked: Vec<Option<&str>> = seen
        .frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Handback(h) => Some(h.request.origin.as_ref().map(|o| o.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [Some(fixture.origin().as_str())]);
    let binds: Vec<&str> = seen
        .frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Bind(b) => Some(b.origin.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(binds, [fixture.origin().as_str()]);
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::Handback, None, None),
            (LogEvent::StepOk, Some(2), None),
            (LogEvent::StepOk, Some(3), None),
            (LogEvent::StepOk, Some(4), None),
        ]
    );
    let named: Vec<(String, String)> = fixture
        .requests()
        .into_iter()
        .map(|r| (r.method, r.path))
        .collect();
    let times = |method: &str, path: &str| {
        named
            .iter()
            .filter(|(m, p)| m == method && p == path)
            .count()
    };
    assert_eq!(
        times("GET", "/sign-in"),
        2,
        "loaded again by GET: {named:?}"
    );
    assert_eq!(
        times("POST", "/signed-in"),
        1,
        "the page's POST in the handback: {named:?}"
    );
    assert_eq!(times("GET", "/read"), 1, "{named:?}");
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// A sign-in page that keeps its sign-in in a cookie `signed` and the localStorage item `token`: where both stand at
/// its load, its script takes the password field out at once and the page reads `in`. Else, at a load with no cookie
/// `seen` it sets that cookie, and at a load that finds it (the handback's load by GET) it sets both 2 s later, as a
/// person's sign-in does, and takes the password field out.
const KEEPS: &str = r#"<!doctype html><title>sign in</title><form onsubmit="return false"><input data-testid="user" aria-label="Email"><input type="password" data-testid="password" aria-label="Password"></form><p data-testid="state">out</p><script>function signedIn() { var p = document.querySelector('[data-testid=password]'); if (p) { p.parentNode.removeChild(p); } document.querySelector('[data-testid=state]').textContent = 'in'; } if (document.cookie.indexOf('signed=1') >= 0 && localStorage.getItem('token') === 'kept-u52') { signedIn(); } else if (document.cookie.indexOf('seen=1') < 0) { document.cookie = 'seen=1; path=/'; } else { setTimeout(function () { document.cookie = 'signed=1; path=/; max-age=3600'; localStorage.setItem('token', 'kept-u52'); signedIn(); }, 2000); }</script>"#;

/// U5.2 (7), A8.3, on a hosted runner alone, headless, two runs: run 1 hands back at the sign-in page, the page signs
/// in by its own script while the handback is open, and after `bound=yes` the run ends `completed` and stores the
/// fixture origin's state by `put_session`. Run 2 gets those parts from its load, imports them when its browser starts,
/// and finds the page signed in: no handback, no `step_failed`, and the run ends `completed`.
#[test]
fn what_run_one_signed_in_serves_run_two() {
    let Some(browser) = hosted("what_run_one_signed_in_serves_run_two") else {
        return;
    };
    let fixture = Fixture::start(&[("/sign-in", KEEPS), ("/read", READ)]);
    let steps = |fixture: &Fixture| {
        vec![
            navigate(1, &fixture.url("/sign-in")),
            handback(2),
            navigate(3, &fixture.url("/read")),
            extract_amount(4, "amount"),
        ]
    };
    let mut one = web_agent(251, &fixture);
    one.steps = steps(&fixture);
    let done = ToRunner::parse(
        br#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#,
    )
    .expect("a handback_reply");
    let bound = ToRunner::parse(br#"{"t":"bind_reply","bound":"yes"}"#).expect("a bind_reply");
    let first = serve_with(
        &one,
        &browser,
        |frame, _| match frame {
            FromRunner::Browser(_) => Act::With(browser_reply()),
            FromRunner::Handback(_) => {
                Act::After(Duration::from_secs(8), Box::new(Act::With(done.clone())))
            }
            FromRunner::Bind(_) => Act::With(bound.clone()),
            _ => Act::Answer,
        },
        Duration::from_secs(120),
    );
    assert_eq!(first.done(), Some(Outcome::Completed), "{}", first.stderr);
    assert_eq!(first.handbacks(), [HandbackReason::SignIn]);
    let kept: Vec<_> = first
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            RunnerCall::PutSession(p) => Some(p),
            _ => None,
        })
        .collect();
    assert!(
        !kept.is_empty() && kept.iter().all(|p| p.origin == fixture.origin()),
        "run 1 keeps the fixture's sign-in: {}",
        first.stderr
    );

    let mut two = web_agent(252, &fixture);
    two.agent = one.agent.clone();
    two.workflow = one.workflow.clone();
    two.config = one.config.clone();
    two.steps = steps(&fixture);
    two.kept = kept;
    let before = fixture.requests().len();
    let second = serve_with(&two, &browser, reply_to_browser, Duration::from_secs(120));
    let loads = second
        .calls()
        .into_iter()
        .filter(|c| matches!(c, RunnerCall::LoadSession(_)))
        .count();
    assert!(loads >= 1, "run 2 loads the kept parts: {}", second.stderr);
    assert_eq!(second.done(), Some(Outcome::Completed), "{}", second.stderr);
    assert!(second.handbacks().is_empty(), "no handback in run 2");
    assert!(
        rows(&second)
            .iter()
            .all(|(event, _, _)| *event == LogEvent::StepOk),
        "{:?}",
        rows(&second)
    );
    let got: Vec<String> = fixture.requests()[before..]
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(
        got.iter().filter(|r| *r == "GET /sign-in").count(),
        1,
        "run 2 loads the sign-in page once: {got:?}"
    );
    assert!(!profile_of(&first, &one.run).0.exists());
    assert!(!profile_of(&second, &two.run).0.exists());
}

/// U4.1w (5), the row `win_single_argument_inside_quotes` of `terminator_table` (crates/halvo-web/src/checks.rs):
/// a browser started with the one argument `--app=x --single-argument`, which the system writes in quotes, before
/// the profile's switch takes that profile directory: the directory exists and holds an entry. The browser gets TMP
/// and TEMP of its own in a scratch directory of the test, in place of the temporary directory of the tests (the
/// coordinator's line 14089).
#[cfg(windows)]
#[test]
fn the_browser_takes_the_profile_after_a_quoted_single_argument() {
    let Some(browser) = hosted("the_browser_takes_the_profile_after_a_quoted_single_argument")
    else {
        return;
    };
    let profile = support::scratch("single-argument").join("profile");
    let temp = support::scratch("single-argument-tmp");
    let mut child = std::process::Command::new(&browser)
        .envs([("TMP", &temp), ("TEMP", &temp)])
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

/// The two browsers of a hosted Mac that the runner finds, in its own order (src/find.rs): Google Chrome, then
/// Microsoft Edge.
const MAC_BROWSERS: [&str; 2] = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
];

/// r3 U5.2e, on the hosted macos job alone (the coordinator's lines 14401, 14450 and 14668): with each of the two
/// browsers of a hosted Mac (`MAC_BROWSERS`), a run whose data directory lies under the hosted runner's own
/// `Library/Application Support`, in a folder of its own whose name the test makes and no other program uses, loads a
/// fixture page and completes with a clean stop. After the stop nothing of the run stands under the runner's
/// `Library/Caches` below that folder's name, where the browser's own rule puts the cache of a profile under
/// `Library/Application Support` when its start names no other cache place: no folder of the run's id, and no file at
/// any depth. For each browser it prints the entries that it finds there (at most `MOST_LINES`, then how many more),
/// each line with the browser's path, its kind and its size. A browser that the hosted Mac does not have fails the case
/// before any run. Its limits: one version of each browser on one system; and at the end of each browser's run it
/// removes, by its own code (`Made`), both folders that it made under the runner's `Library`, with all that they hold,
/// also when it fails; a hosted runner is thrown away after the job.
#[test]
fn a_run_under_the_persons_library_leaves_no_cache_outside_its_profile() {
    const TEST: &str = "a_run_under_the_persons_library_leaves_no_cache_outside_its_profile";
    if hosted_on_mac(TEST).is_none() {
        return;
    }
    for browser in MAC_BROWSERS {
        assert!(
            std::path::Path::new(browser).is_file(),
            "{TEST}: the hosted Mac has no {browser}; the case runs with Google Chrome and with Microsoft Edge"
        );
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME is set"));
    let made_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock")
        .as_nanos();
    let mut faults = Vec::new();
    for (n, browser) in [(251, MAC_BROWSERS[0]), (252, MAC_BROWSERS[1])] {
        let name = format!(
            "halvo-u52e-cache-place-{}-{made_at}-{n}",
            std::process::id()
        );
        faults.extend(cache_case(TEST, &home, &name, n, browser));
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

/// One run of the cache case with `browser`, in the folder `name` under the runner's `Library/Application Support`
/// and with the run number `n`: what fails, each fault named by the browser's path. Both folders that it made under
/// the runner's `Library` are removed when it returns.
fn cache_case(
    test: &'static str,
    home: &std::path::Path,
    name: &str,
    n: u8,
    browser: &str,
) -> Vec<String> {
    let data = home.join("Library/Application Support").join(name);
    let caches = home.join("Library/Caches").join(name);
    if std::fs::symlink_metadata(&data).is_ok() || std::fs::symlink_metadata(&caches).is_ok() {
        return vec![format!(
            "{browser}: the folder {name} stands before the run"
        )];
    }
    let _made = Made {
        test,
        dirs: vec![data.clone(), caches.clone()],
    };
    std::fs::create_dir(&data).expect("make the data directory");
    let fixture = Fixture::start(&[("/read", READ)]);
    let mut sup = web_agent(n, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/read")),
        extract_amount(2, "amount"),
    ];
    let seen = serve_in(
        &sup,
        std::path::Path::new(browser),
        &data,
        reply_to_browser,
        LONG,
    );
    let entries = entries_below(&caches);
    print_entries(test, &format!("{browser}: Library/Caches/{name}"), &entries);
    let run_folder = caches
        .join("agents")
        .join("profiles")
        .join(sup.run.as_str());
    let (run_dir, _) = profile_of(&seen, &sup.run);
    let files: Vec<&str> = entries
        .iter()
        .filter(|e| !e.dir)
        .map(|e| e.rel.as_str())
        .collect();
    let checks = [
        (
            seen.done() == Some(Outcome::Completed),
            format!("the run did not complete: {}", seen.stderr),
        ),
        (
            rows(&seen)
                == [
                    (LogEvent::StepOk, Some(1), None),
                    (LogEvent::StepOk, Some(2), None),
                ],
            format!("the rows are {:?}", rows(&seen)),
        ),
        (!run_dir.exists(), format!("{} is left", run_dir.display())),
        (
            fixture
                .requests()
                .iter()
                .any(|r| r.method == "GET" && r.path == "/read"),
            "the page was not loaded".to_string(),
        ),
        (
            files.is_empty(),
            format!("files of the run stand under Library/Caches/{name}: {files:?}"),
        ),
        (
            std::fs::symlink_metadata(&run_folder).is_err(),
            format!(
                "the run's folder stands under Library/Caches: {}",
                run_folder.display()
            ),
        ),
        (
            seen.stderr.contains("run browser=stopped removed=yes"),
            format!("the stop did not say removed=yes: {}", seen.stderr),
        ),
    ];
    checks
        .into_iter()
        .filter(|(held, _)| !held)
        .map(|(_, fault)| format!("{browser}: {fault}"))
        .collect()
}

/// A payment page: the amount `12.34` and a form that posts a token to `/charge` by its button `pay-now`.
const PAY: &str = r#"<!doctype html><title>pay</title><p>Amount <span data-testid="amount">12.34</span></p><form method="post" action="/charge"><input type="hidden" name="token" value="halvo-runner-pay-token"><button data-testid="pay-now">Pay now</button></form>"#;

/// The page after the charge, which names the amount charged.
const CHARGED: &str = r#"<!doctype html><title>paid</title><h1>Payment received</h1><p>Amount charged <span data-testid="amount-charged">12.34</span></p>"#;

/// r3 U4.1w-2 (7), on the hosted macos job alone: in a real browser the POST of the fixture's payment form, which the
/// web step of the class `payment` makes, is held while the person is asked; after the test's `allow_once` it leaves
/// once, the fixture answers it 200 with a page that names the amount that the card showed, and the run ends
/// `completed` with the effect `committed`. ONE payment stands in the fixture's list: one `POST /charge`, with the
/// form's token.
#[test]
fn a_payment_leaves_once_after_the_yes() {
    let test = "a_payment_leaves_once_after_the_yes";
    if cfg!(windows) {
        say(
            test,
            "not run: it starts a real browser, on the hosted macos job alone",
        );
        return;
    }
    let Some(browser) = hosted(test) else {
        return;
    };
    let fixture = Fixture::start(&[("/pay", PAY), ("/charge", CHARGED)]);
    let mut sup = web_agent(253, &fixture);
    sup.slots = vec![
        slot("payee", "Larkspur Power"),
        slot("instrument", "Card ending 4417"),
    ];
    sup.steps = vec![
        navigate(1, &fixture.url("/pay")),
        extract_amount(2, "amount"),
        pay_click(3, "pay-now", fixture.origin().as_str()),
    ];
    let seen = serve_with(
        &sup,
        &browser,
        |frame, _| match frame {
            FromRunner::Browser(_) => Act::With(browser_reply()),
            FromRunner::Confirm(_) => decision(frame, |_| Answer::AllowOnce),
            _ => Act::Answer,
        },
        LONG,
    );
    let charges: Vec<_> = fixture
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(
        charges.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        ["/charge"],
        "one payment: rows {:?}, states {:?}, stderr {}",
        rows(&seen),
        seen.states(),
        seen.stderr
    );
    assert!(charges[0]
        .bytes
        .windows(b"halvo-runner-pay-token".len())
        .any(|w| w == b"halvo-runner-pay-token"));
    assert_eq!(
        seen.frames
            .iter()
            .filter(|f| matches!(f, FromRunner::Confirm(_)))
            .count(),
        1,
        "one card"
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ],
        "{}",
        seen.stderr
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// The value of the password field of `MARKS`.
const WORD: &str = "halvo-runner-word-marker";

/// A password field `word`, a credential field by its type alone, that holds `WORD`, and a plain field `name`, each of
/// which asks its own origin at each input (`/typed-word`, `/typed-name`); and a script that asks `/switch` every
/// 100 ms. At the word `mark` the script deletes the property `__halvoSign` of the password field, sets it to the
/// empty text, sets the field's `type` to `text`, and asks `/marked`; at the word `forge` it sets `__halvoSign` to
/// `secret` on the plain field, hides the password field (`display: none`) and asks `/forged`; each once.
const MARKS: &str = r#"<!doctype html><title>marks</title><input type="password" data-testid="word" aria-label="Word" value="halvo-runner-word-marker" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed-word', false); x.send(); } catch (e) {}"><input data-testid="name" aria-label="Name" oninput="var x = new XMLHttpRequest(); try { x.open('GET', '/typed-name', false); x.send(); } catch (e) {}"><script>(function () { var word = document.querySelector('[data-testid=word]'); var plain = document.querySelector('[data-testid=name]'); var did = {}; var busy = false; function tell(path) { var x = new XMLHttpRequest(); x.open('GET', path); x.send(); } setInterval(function () { if (busy) { return; } busy = true; var x = new XMLHttpRequest(); x.onloadend = function () { busy = false; var said = x.status === 200 ? x.responseText : ''; if (said === 'mark' && !did.mark) { did.mark = true; delete word.__halvoSign; word.__halvoSign = ''; word.type = 'text'; tell('/marked'); } if (said === 'forge' && !did.forge) { did.forge = true; plain.__halvoSign = 'secret'; word.style.display = 'none'; tell('/forged'); } }; x.open('GET', '/switch'); x.send(); }, 100); })();</script>"#;

/// r3 U5.13, A8.2, the coordinator's ruling of log line 14127 (2): the mark of a credential field lives in the
/// executor's own script world, which the page's scripts cannot reach. A handback loads the page again before it
/// opens, so the readings of one document are those after each `done` of one handback: the reading after the first
/// `done` marks the password field; then, at the second handback, the page's own script deletes the property
/// `__halvoSign` of the password field, sets it to the empty text and sets the field's `type` to `text`, and the test
/// answers `done` only after the page has said so (`/marked`). The reading after that still hands back (`sign_in`)
/// while the password field shows as a field of the type text. At that third handback the page's script sets
/// `__halvoSign` to `secret` on the plain field and hides the password field (`/forged`), and the reading after it
/// finds no credential field and binds. Then step 2's fill of the plain field acts (the fixture sees `/typed-name`):
/// the page's own `__halvoSign` gave it no sign; and step 3, whose `pre` reads the value of the hidden password field,
/// is refused (`step_failed`), with no act on that field and its value in no frame and no line of the runner. With the
/// base's cdp.rs, whose rule kept the mark in the page's world, the test fails at one of its two assertions on the
/// mark: at the third handback where the reading after the script finds no sign on the password field, else at the
/// fill of the plain field, which the page's own mark makes a credential field there.
#[test]
fn a_page_s_script_cannot_take_the_mark_off_a_password_field() {
    let Some(browser) = hosted("a_page_s_script_cannot_take_the_mark_off_a_password_field") else {
        return;
    };
    let (fixture, switch) = Fixture::start_switched(&[("/marks", MARKS)], "/switch");
    let mut sup = web_agent(213, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/marks")),
        step(2, "fill", Some("name"), &[konst("text", "Ada")], &[], &[]),
        step(
            3,
            "wait",
            None,
            &[konst("ms", "1")],
            &[cond("value_is", Some("word"), Some(WORD))],
            &[],
        ),
    ];
    let done = ToRunner::parse(
        br#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#,
    )
    .expect("a handback_reply");
    let bound = ToRunner::parse(br#"{"t":"bind_reply","bound":"yes"}"#).expect("a bind_reply");
    let asked = |path: &str| fixture.requests().iter().any(|r| r.path == path);
    let after = |word: &str, path: &str| {
        *switch.lock().expect("the switch") = word.to_string();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !asked(path) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        asked(path)
    };
    let (mut marked, mut forged) = (false, false);
    let seen = serve_with(
        &sup,
        &browser,
        |frame, before| match frame {
            FromRunner::Browser(_) => Act::With(browser_reply()),
            FromRunner::Handback(_) => {
                match before
                    .iter()
                    .filter(|f| matches!(f, FromRunner::Handback(_)))
                    .count()
                {
                    0 => Act::With(done.clone()),
                    1 => {
                        marked = after("mark", "/marked");
                        Act::With(done.clone())
                    }
                    2 => {
                        forged = after("forge", "/forged");
                        Act::With(done.clone())
                    }
                    _ => Act::Answer,
                }
            }
            FromRunner::Bind(_) => Act::With(bound.clone()),
            _ => Act::Answer,
        },
        Duration::from_secs(150),
    );
    let typed = |path: &str| fixture.requests().iter().filter(|r| r.path == path).count();
    assert!(
        marked,
        "the page's script did not take the mark off at the second handback; {}",
        seen.stderr
    );
    assert_eq!(
        seen.handbacks().get(..3),
        Some(
            &[
                HandbackReason::SignIn,
                HandbackReason::SignIn,
                HandbackReason::SignIn
            ][..]
        ),
        "the reading after the page's script took the mark off the password field did not hand back: handbacks {:?}, \
         rows {:?}, {}",
        seen.handbacks(),
        rows(&seen),
        seen.stderr
    );
    assert!(
        forged,
        "the page's script did not mark the plain field at the third handback; {}",
        seen.stderr
    );
    assert!(
        typed("/typed-name") > 0,
        "the plain field on which the page's script set __halvoSign to secret was read as a credential field: no step \
         filled it; handbacks {:?}, rows {:?}, {}",
        seen.handbacks(),
        rows(&seen),
        seen.stderr
    );
    assert_eq!(
        typed("/typed-word"),
        0,
        "a step acted on the password field whose mark the page's script took off"
    );
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::Handback, None, None),
            (LogEvent::Handback, None, None),
            (LogEvent::Handback, None, None),
            (LogEvent::StepOk, Some(2), None),
            (LogEvent::StepFailed, Some(3), Some(Cause::StepFailed)),
            (LogEvent::Handback, None, None),
            (LogEvent::Stopped, None, Some(Cause::StepFailed)),
        ],
        "step 3, whose pre reads the value of the password field whose mark the page's script took off, was not \
         refused: {}",
        seen.stderr
    );
    assert_eq!(
        seen.handbacks(),
        [
            HandbackReason::SignIn,
            HandbackReason::SignIn,
            HandbackReason::SignIn,
            HandbackReason::StepFailed
        ]
    );
    let binds: Vec<&str> = seen
        .frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Bind(b) => Some(b.origin.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(binds, [fixture.origin().as_str()]);
    assert_eq!(seen.done(), Some(Outcome::HandedBack), "{}", seen.stderr);
    assert!(!seen.stderr.contains(WORD), "{}", seen.stderr);
    for frame in &seen.frames {
        let json = frame.to_json().expect("a frame");
        assert!(
            !json.contains(WORD),
            "a frame holds the password field's value"
        );
    }
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// The heading of the fixture's payment frame, behind which the payment forwarder of
/// `a_run_reads_and_selects_inside_the_payment_frame` puts `CLICK_MARK`.
const HEADING: &str = "<h1>Payment</h1>";

/// A listener of the test in the payment frame: a click that arrives at the amount adds one element with the test id
/// `halvo-clicked`, which the click step's post condition finds.
const CLICK_MARK: &str = r#"<script>document.addEventListener('click', function (e) { if (e.target.closest && e.target.closest('[data-testid="amount"]')) { var p = document.createElement('p'); p.setAttribute('data-testid', 'halvo-clicked'); p.textContent = 'clicked'; document.body.appendChild(p); } }, true);</script>"#;

/// U5.10-F (the coordinator's ruling of log line 15706, and its word on C2: an agent can click and read inside a
/// frame that its step names): on the product's own framed fixture, the page `/pay` of the utility site with its
/// payment frame of another origin, a run clicks the amount inside the frame, reads it and selects the payment
/// method inside it, each target found by the frame's origin that its step names, and each step's post condition
/// read in the frame: the click's by the mark of a listener of the test that only a click arriving at the amount
/// adds. No step is of an effect class, so nothing changes state at the fixture. The browser reaches the fixture's
/// two hosts by two forwarders of the test on the loopback address: the utility one carries the fixture's own
/// sign-in and writes the frame's address as the payment forwarder's, so that the frame is of another origin than
/// the page, and the payment one puts the listener into the frame's page; the bindings are the two forwarders'
/// origins.
#[test]
fn a_run_reads_and_selects_inside_the_payment_frame() {
    const TEST: &str = "a_run_reads_and_selects_inside_the_payment_frame";
    let Some(browser) = hosted(TEST) else {
        return;
    };
    let fixture = support::connect::Fixture::start();
    fixture.reset(1, &[]);
    let (_, port) =
        halvo_web::cdp::host_port(fixture.origin("utility").as_str()).expect("the fixture's port");
    let cookie = fixture_sign_in(port);
    let pay = Forwarder::start(
        port,
        "pay.agent.test",
        None,
        Some((HEADING.to_string(), format!("{HEADING}{CLICK_MARK}"))),
    );
    let utility = Forwarder::start(
        port,
        "utility.agent.test",
        Some(cookie),
        Some((format!("http://pay.agent.test:{port}"), pay.url(""))),
    );
    let seed: serde_json::Value = serde_json::from_str(SEED_1).expect("seed 1");
    let minor = seed["utility"]["amount_due"]
        .as_u64()
        .expect("the amount due");
    let amount = format!("{}.{:02}", minor / 100, minor % 100);
    let method = seed["utility"]["instruments"][1]
        .as_str()
        .expect("a second instrument")
        .to_string();
    let frame = pay.origin();
    let mut sup = Supervisor::new(254, Category::UtilityBillPayment, Vec::new());
    sup.origins = vec![utility.origin(), pay.origin()];
    sup.steps = vec![
        navigate(1, &utility.url("/pay")),
        framed_step(
            2,
            "click",
            "amount",
            &[&frame],
            &[],
            &[framed_cond("present", "halvo-clicked", &[&frame], None)],
        ),
        framed_step(
            3,
            "extract",
            "amount",
            &[&frame],
            &[konst("as", "amount"), konst("parse", "amount")],
            &[framed_cond("text_is", "amount", &[&frame], Some(&amount))],
        ),
        framed_step(
            4,
            "select",
            "payment-method",
            &[&frame],
            &[konst("option", &method)],
            &[framed_cond(
                "value_is",
                "payment-method",
                &[&frame],
                Some(&method),
            )],
        ),
    ];
    let changing = |state: &serde_json::Value| state["requests"]["state_changing"].as_u64();
    let before = changing(&fixture.state());
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    let got = rows(&seen);
    say(TEST, &format!("rows {got:?}"));
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        got,
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::StepOk, Some(2), None),
            (LogEvent::StepOk, Some(3), None),
            (LogEvent::StepOk, Some(4), None)
        ]
    );
    let framed = pay.requests();
    assert!(
        framed
            .iter()
            .any(|(m, p)| m == "GET" && p.starts_with("/frame?kind=bill&token=")),
        "the frame's document came through the payment forwarder: {framed:?}"
    );
    let state = fixture.state();
    assert_eq!(changing(&state), before, "{state}");
    assert_eq!(
        state["utility"]["payments"].as_array().map(Vec::len),
        Some(0),
        "{state}"
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

/// A page that asks for a sign-in by a button alone: no field, a paragraph and a button `Sign in`.
const BUTTON_ALONE: &str = r#"<!doctype html><title>bills</title><p>Your bills are behind a sign-in.</p><button type="button" data-testid="sign-in">Sign in</button>"#;

/// r3 U5.14, the coordinator's rulings of log lines 14127 (1) and 16106, on a hosted runner alone: on a page that
/// shows no credential field and asks for a sign-in by a button alone, a click on a control that the page does not
/// have fails, and the run hands back with the reason `sign_in` in place of the failure: the second reading reads the
/// button by the role and the name that the browser's accessibility tree gives it. The request names the fixture's
/// origin; the supervisor declines, and the run ends `declined` with no row of a failed step. With the base's web.rs,
/// which makes no second reading, the step fails, the run hands back with `step_failed` and ends `handed_back`, and
/// the case fails at its first assertion, on the handback.
#[test]
fn a_failed_step_on_a_page_that_asks_by_a_button_alone_hands_back() {
    let Some(browser) = hosted("a_failed_step_on_a_page_that_asks_by_a_button_alone_hands_back")
    else {
        return;
    };
    let fixture = Fixture::start(&[("/bills", BUTTON_ALONE)]);
    let mut sup = web_agent(214, &fixture);
    sup.steps = vec![
        navigate(1, &fixture.url("/bills")),
        step(2, "click", Some("pay-bill"), &[], &[], &[]),
    ];
    let seen = serve_with(&sup, &browser, reply_to_browser, LONG);
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::SignIn],
        "U5.14: the failed step did not hand back with the reason sign_in: outcome {:?}, rows {:?}, {}",
        seen.done(),
        rows(&seen),
        seen.stderr
    );
    assert_eq!(
        rows(&seen),
        [
            (LogEvent::StepOk, Some(1), None),
            (LogEvent::Handback, None, None),
            (LogEvent::Stopped, None, Some(Cause::Declined)),
        ],
        "{}",
        seen.stderr
    );
    assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
    let origins: Vec<Option<&str>> = seen
        .frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Handback(h) => Some(h.request.origin.as_ref().map(|o| o.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(origins, [Some(fixture.origin().as_str())]);
    assert!(!profile_of(&seen, &sup.run).0.exists());
}

use halvo_schema::executable::names::FieldName;
use halvo_schema::executable::{ExecutableStep, WebOrigin};
use support::browser::{framed_pay_click, Hang};

/// The heading of the fixture's page after a recorded `POST /charge` of the kind `bill`.
const RECEIVED: &str = "<h1>Payment received</h1>";

/// What a run of `framed_payment` leaves: what the supervisor saw, the fixture's state after the run and right before
/// the runner started (after the test's sign-in), the origin of the page `/pay` and of its payment frame, and the
/// playbook that the store served the run.
struct Paid {
    seen: Seen,
    state: serde_json::Value,
    before: serde_json::Value,
    top: WebOrigin,
    frame: WebOrigin,
    steps: Vec<ExecutableStep>,
}

/// r3 U5.10-B part two: a run of the supervisor `n` on the product's own framed fixture, the page `/pay` of the
/// utility site with its payment frame of another origin, reached by the frame unit's two forwarders on the loopback
/// address (the utility one carries the fixture's own sign-in and writes the frame's address as the payment
/// forwarder's): step 1 opens `/pay`, step 2 reads the amount in the payment frame, and step 3 clicks `pay-now` in
/// that frame with the effect `request`, of the class `payment`, with the conditions `post` that `post` gives for the
/// frame's origin; the person answers `allow_once`. The payment forwarder rewrites the frame's pages by `rewrite`
/// where one is given, and `bound` joins the bindings beside the two forwarders' origins. The fixture's reset turns
/// on `variants` (r3 U5.10-C stage three).
fn framed_payment(
    browser: &std::path::Path,
    n: u8,
    post: impl Fn(&WebOrigin) -> Vec<String>,
    rewrite: Option<(String, String)>,
    bound: &[WebOrigin],
    variants: &[&str],
) -> Paid {
    let fixture = support::connect::Fixture::start();
    fixture.reset(1, variants);
    let (_, port) =
        halvo_web::cdp::host_port(fixture.origin("utility").as_str()).expect("the fixture's port");
    let cookie = fixture_sign_in(port);
    let pay = Forwarder::start(port, "pay.agent.test", None, rewrite);
    let utility = Forwarder::start(
        port,
        "utility.agent.test",
        Some(cookie),
        Some((format!("http://pay.agent.test:{port}"), pay.url(""))),
    );
    let seed: serde_json::Value = serde_json::from_str(SEED_1).expect("seed 1");
    let text = |value: &serde_json::Value| value.as_str().expect("a text").to_string();
    let (top, frame) = (utility.origin(), pay.origin());
    let mut sup = Supervisor::new(
        n,
        Category::UtilityBillPayment,
        vec![
            slot("payee", &text(&seed["utility"]["payee"])),
            slot("instrument", &text(&seed["utility"]["instruments"][0])),
        ],
    );
    sup.origins = [top.clone(), frame.clone()]
        .into_iter()
        .chain(bound.iter().cloned())
        .collect();
    sup.steps = vec![
        navigate(1, &utility.url("/pay")),
        framed_step(
            2,
            "extract",
            "amount",
            &[&frame],
            &[konst("as", "amount"), konst("parse", "amount")],
            &[],
        ),
        framed_pay_click(3, "pay-now", &[&frame], frame.as_str(), &post(&frame)),
    ];
    let before = fixture.state();
    let seen = serve_with(
        &sup,
        browser,
        |f, _| match f {
            FromRunner::Browser(_) => Act::With(browser_reply()),
            FromRunner::Confirm(_) => decision(f, |_| Answer::AllowOnce),
            _ => Act::Answer,
        },
        LONG,
    );
    assert!(!profile_of(&seen, &sup.run).0.exists());
    Paid {
        seen,
        state: fixture.state(),
        before,
        top,
        frame,
        steps: sup.steps,
    }
}

/// The amounts of the payments that the fixture recorded, in minor units, and seed 1's amount due.
fn payments(paid: &Paid) -> (Vec<u64>, u64) {
    let seed: serde_json::Value = serde_json::from_str(SEED_1).expect("seed 1");
    let due = seed["utility"]["amount_due"]
        .as_u64()
        .expect("the amount due");
    let made = paid.state["utility"]["payments"]
        .as_array()
        .expect("the payments")
        .iter()
        .map(|p| p["amount"].as_u64().expect("an amount"))
        .collect();
    (made, due)
}

/// r3 U5.10-B part two, sentence 1: on the product's framed fixture, an effect step's condition `post` is read after
/// the release of its request and its reply, on the frame's page after the charge: the condition `present` of
/// `amount-charged` in the payment frame, a control that only that page holds, holds, the one payment is made at
/// the amount due, the effect is `committed` and the run `completed`.
#[test]
fn an_effect_step_reads_its_post_conditions_after_the_release() {
    const TEST: &str = "an_effect_step_reads_its_post_conditions_after_the_release";
    let Some(browser) = hosted(TEST) else {
        return;
    };
    let paid = framed_payment(
        &browser,
        255,
        |frame| vec![framed_cond("present", "amount-charged", &[frame], None)],
        None,
        &[],
        &[],
    );
    let got = rows(&paid.seen);
    say(TEST, &format!("rows {got:?}"));
    assert_eq!(
        paid.seen.done(),
        Some(Outcome::Completed),
        "{}",
        paid.seen.stderr
    );
    assert!(got.contains(&(LogEvent::StepOk, Some(3), None)), "{got:?}");
    assert!(
        paid.seen
            .stderr
            .contains("run effect=made page=read refused=no post=held"),
        "{}",
        paid.seen.stderr
    );
    assert_eq!(
        paid.seen.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let (made, due) = payments(&paid);
    assert_eq!(made, [due], "{}", paid.state);
}

/// r3 U5.10-B part two, sentence 2, and the coordinator's points (3) and (4) of log line 16745: the read after the
/// reply follows the frame of the held request, so the amount that the payment frame's page names after the charge is
/// read there, the amount due, and the run is `completed` with no row of the read after the reply; the card names the
/// frame document's origin for the amount that it read in the frame, and the top page's origin stands in the playbook
/// that the store served the run, as the address of the step that opened the page.
#[test]
fn the_read_after_the_reply_follows_the_frame_of_the_held_request() {
    const TEST: &str = "the_read_after_the_reply_follows_the_frame_of_the_held_request";
    let Some(browser) = hosted(TEST) else {
        return;
    };
    let paid = framed_payment(&browser, 249, |_| Vec::new(), None, &[], &[]);
    let got = rows(&paid.seen);
    say(TEST, &format!("rows {got:?}"));
    assert_eq!(
        paid.seen.done(),
        Some(Outcome::Completed),
        "{}",
        paid.seen.stderr
    );
    assert!(
        !got.iter().any(|(_, _, cause)| matches!(
            cause,
            Some(Cause::PageUnread | Cause::AmountUnread | Cause::AmountDiffers)
        )),
        "{got:?}"
    );
    assert!(
        paid.seen.stderr.contains("run effect=made page=read"),
        "{}",
        paid.seen.stderr
    );
    let (made, due) = payments(&paid);
    assert_eq!(made, [due], "{}", paid.state);
    let read_in: Vec<Option<&str>> = paid
        .seen
        .frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Confirm(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| &c.request.effects)
        .map(|e| {
            e.bound
                .field(FieldName::Amount)
                .and_then(|f| f.origin.as_ref())
                .map(WebOrigin::as_str)
        })
        .collect();
    let opened = paid.steps[0]
        .args
        .iter()
        .find(|a| a.name.as_str() == "url")
        .and_then(|a| a.value.as_ref())
        .and_then(|url| WebOrigin::of_address(url.as_str()).ok());
    assert_eq!(
        (read_in, opened),
        (vec![Some(paid.frame.as_str())], Some(paid.top.clone())),
        "the frame document's origin on the card, the top page's in the playbook"
    );
}

/// r3 U5.10-B part two, sentence 3: a made payment whose page cannot be read, the frame's page after the charge with
/// an image of a listener of the test that never answers, so that its document never completes within the step's
/// time, writes `page_unread` in the step's `stopped` row and ends the run `failed` with it; the one payment is made
/// at the amount due and its effect stays `committed`.
#[test]
fn a_made_payment_whose_page_cannot_be_read_ends_the_run_with_a_cause_of_its_own() {
    const TEST: &str =
        "a_made_payment_whose_page_cannot_be_read_ends_the_run_with_a_cause_of_its_own";
    let Some(browser) = hosted(TEST) else {
        return;
    };
    let hang = Hang::start();
    let paid = framed_payment(
        &browser,
        247,
        |_| Vec::new(),
        Some((
            RECEIVED.to_string(),
            format!(r#"{RECEIVED}<img src="{}" alt="">"#, hang.url("/never")),
        )),
        &[hang.origin()],
        &[],
    );
    let got = rows(&paid.seen);
    say(TEST, &format!("rows {got:?}"));
    assert_eq!(
        paid.seen.done(),
        Some(Outcome::Failed),
        "{}",
        paid.seen.stderr
    );
    assert!(
        got.contains(&(LogEvent::Stopped, Some(3), Some(Cause::PageUnread))),
        "{got:?}"
    );
    assert_eq!(
        got.last(),
        Some(&(LogEvent::Stopped, None, Some(Cause::PageUnread))),
        "{got:?}"
    );
    assert_eq!(
        paid.seen.states().last(),
        Some(&EffectState::Committed),
        "{}",
        paid.seen.stderr
    );
    let (made, due) = payments(&paid);
    assert_eq!(made, [due], "{}", paid.state);
    assert!(
        hang.taken() >= 1,
        "the page asked for the image that never comes"
    );
}

/// r3 U5.10-C stage three (the coordinator's log lines 16169 and 17302): on the product's framed fixture, the held
/// request of the payment frame's form leaves only where the amount that its form names is the card's. (a) With the
/// variant `form_amount` the form's hidden field `amount` carries the amount that the frame shows, the amount that the
/// card read: one card is shown, and after the yes the payment leaves once, recorded at the amount that the token
/// stands for. (b) With `form_amount_other` the field carries that amount with 1.00 added while the frame still shows
/// the token's amount: the held request is failed before any card, so no confirm frame and no effect row stand, the
/// step's row says `step_failed` with `amount_differs` and no row holds a `page_amount`, the run ends `failed` with it,
/// the fixture records no payment, and its count of state-changing requests is the same before and after the run.
#[test]
fn a_held_request_leaves_only_where_the_amount_that_its_form_names_is_the_cards() {
    const TEST: &str =
        "a_held_request_leaves_only_where_the_amount_that_its_form_names_is_the_cards";
    let Some(browser) = hosted(TEST) else {
        return;
    };
    let cards = |paid: &Paid| {
        paid.seen
            .frames
            .iter()
            .filter(|f| matches!(f, FromRunner::Confirm(_)))
            .count()
    };

    let paid = framed_payment(&browser, 238, |_| Vec::new(), None, &[], &["form_amount"]);
    let got = rows(&paid.seen);
    say(TEST, &format!("form_amount rows {got:?}"));
    assert_eq!(
        paid.seen.done(),
        Some(Outcome::Completed),
        "{}",
        paid.seen.stderr
    );
    assert_eq!(cards(&paid), 1, "one card: {}", paid.seen.stderr);
    assert_eq!(
        paid.seen.states(),
        [
            EffectState::Held,
            EffectState::Approved,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let (made, due) = payments(&paid);
    assert_eq!(made, [due], "{}", paid.state);

    let paid = framed_payment(
        &browser,
        239,
        |_| Vec::new(),
        None,
        &[],
        &["form_amount_other"],
    );
    let got = rows(&paid.seen);
    say(TEST, &format!("form_amount_other rows {got:?}"));
    assert_eq!(
        paid.seen.done(),
        Some(Outcome::Failed),
        "{}",
        paid.seen.stderr
    );
    assert_eq!(cards(&paid), 0, "no card: {}", paid.seen.stderr);
    assert!(paid.seen.effects().is_empty(), "no effect row: {got:?}");
    assert!(
        got.contains(&(LogEvent::StepFailed, Some(3), Some(Cause::AmountDiffers))),
        "{got:?}"
    );
    assert_eq!(
        got.last(),
        Some(&(LogEvent::Stopped, None, Some(Cause::AmountDiffers))),
        "{got:?}"
    );
    assert!(
        paid.seen.logs().iter().all(|l| l.page_amount.is_none()),
        "no page_amount: {got:?}"
    );
    assert!(
        paid.seen
            .stderr
            .contains("run effect=failed at=body reason=amount_differs"),
        "{}",
        paid.seen.stderr
    );
    let (made, _) = payments(&paid);
    assert!(made.is_empty(), "no payment: {}", paid.state);
    assert_eq!(
        paid.state["requests"]["state_changing"], paid.before["requests"]["state_changing"],
        "no state-changing request reached the fixture during the run: {} before {}",
        paid.state, paid.before
    );
}
