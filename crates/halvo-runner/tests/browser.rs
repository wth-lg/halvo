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
    bearing, cond, extract_amount, from_slot, handback, hosted, hosted_headed, hosted_mac, konst,
    navigate, own_start_us, pay_click, print_outside_the_profile, profile_of, say, serve_finding,
    serve_with, step, Fixture,
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
/// frame's document and moved into itself meets the reading, which takes an element that the page's world does not
/// know as one of its own for a credential field: a handback of the reason `sign_in`, declined, no click and no value
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
