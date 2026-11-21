//! `agent-gate privacy` (contracts.md A14.4): the checks of this run's records that this build makes, on the derived
//! store, opened with a key under the owner's step list (`halvo_owner::steps::list`), whose handlers store the rows
//! of the family `agent` as the product stores them.
//!
//! The play (`play`) proposes two workflows from summaries, approves both, and gives each an agent: `create_agent`,
//! `set_bindings` and `start_run` on `coordinator_write`, and `put_session` on `runner`. The first workflow's
//! bindings hold the password marker and the token marker of A10.4, and its stored session holds the page marker in
//! base64. Then the play reads every row of the family through the store, makes the one decision `delete` of the
//! first workflow (`Store::decide`), and reads every row again. The gate writes no delete code: the store's delete
//! cascade removes the rows.
//!
//! The gate reads the rows through a handler of the family `agent` under its own further role (`READ_ROLE`), which
//! reads every kind of the family by `HandlerCx::rows`, the store's own read with its key; and through a delete guard
//! of the family, which the store calls inside the transaction of the delete, before it removes a row, with the rows
//! of the family held under the workflow. The guard answers `GuardAnswer::Go`.
//!
//! The scrub of the file's pages is the store's promise, and no byte search of this unit shows it: `agent_scrub` reads
//! rows through the store, not the bytes of its files.
//!
//! `agent_delete_all` prints `not-run`: the removal of `agents/` by `halvo-capture --delete-all` is shown by
//! halvo-capture's own test of the walk, not by this command. `agent_listed`, `agent_outside` and `agent_expired` of
//! A14.4 are not built.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use halvo_derived::{
    Clock, DbKey, GuardAnswer, Handler, HandlerCx, IdSource, Report, Store, Takes,
};
use halvo_schema::category::SeedCategory;
use halvo_schema::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
use halvo_schema::kinds::{registry, KindEntry};
use halvo_schema::record::{
    Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text, Workflow,
};
use halvo_schema::request::{
    self, Decoded, ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, RawRequest,
    ReplyResult,
};
use halvo_schema::site::AppId;
use halvo_schema::RowRead;
use serde_json::value::RawValue;

/// The family of this run's rows (A7.3).
const FAMILY: &str = "agent";

/// The further role under which the gate reads every row of the family.
pub const READ_ROLE: &str = "agent_gate_read";

/// The three markers of A10.4 that privacy checks search for: the password, the token and the page marker of seed 1.
pub const MARKERS: [&str; 3] = [
    "Zq7-halvo-marker-pw",
    "Zq7-halvo-marker-token",
    "Zq7-halvo-marker-page-1",
];

const DAY_NS: i64 = 86_400_000_000_000;
const DAY: u32 = 20_767;

/// The apps of the two workflows' summaries; each app makes a key of its own, so the step proposes one workflow for
/// each.
const FIRST_APP: &str = "com.example.browser";
const SECOND_APP: &str = "com.example.mail";

/// The checks of A14.4 that the command prints, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Deleted,
    Scrub,
    DeleteAll,
}

impl Check {
    pub const ALL: [Check; 3] = [Check::Deleted, Check::Scrub, Check::DeleteAll];

    pub fn name(self) -> &'static str {
        match self {
            Check::Deleted => "agent_deleted",
            Check::Scrub => "agent_scrub",
            Check::DeleteAll => "agent_delete_all",
        }
    }
}

/// The result of one check: `not-run` for a check that this build does not make, which the gate reads as not
/// verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Miss,
    NotRun,
}

impl Outcome {
    fn holds(holds: bool) -> Outcome {
        if holds {
            Outcome::Ok
        } else {
            Outcome::Miss
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Miss => "miss",
            Outcome::NotRun => "not-run",
        }
    }
}

/// A stored row: its kind and its serialized text, as the store hands it out.
pub type Row = (String, String);

/// What the play saw of the one delete.
#[derive(Clone, Debug)]
pub struct Delete {
    /// The deleted workflow.
    pub first: WorkflowId,
    /// The workflow that stays.
    pub second: WorkflowId,
    /// The first workflow was listed by the store's interface read before the delete.
    pub stored: bool,
    /// Every row of the family that the store's keyed read handed out right before the delete call.
    pub before: Vec<Row>,
    /// The same read right after it.
    pub after: Vec<Row>,
    /// Each call of the delete guard during the delete call: the workflow and the rows of the family held under it.
    pub guarded: Vec<(WorkflowId, Vec<Row>)>,
    /// The reports that the store handed its host during the delete call, one for each committed transaction.
    pub reports: usize,
    /// The first of those reports raises the revision of the family above the last report before the call.
    pub family_rose: bool,
}

/// Whether the row names the workflow: the value under its kind's workflow key is the workflow's id, or its text
/// holds the id as a JSON string.
pub fn names(row: &Row, id: &WorkflowId) -> bool {
    let quoted = format!("\"{}\"", id.as_str());
    if row.1.contains(&quoted) {
        return true;
    }
    let Some(key) = registry()
        .find(|entry| entry.name == row.0)
        .and_then(|entry| entry.workflow_key)
    else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&row.1)
        .ok()
        .is_some_and(|value| value[key].as_str() == Some(id.as_str()))
}

/// The rows among `rows` that name the workflow.
pub fn naming<'r>(rows: &'r [Row], id: &WorkflowId) -> Vec<&'r Row> {
    rows.iter().filter(|row| names(row, id)).collect()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let at = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The forms in which a row's text can hold a marker: the marker itself, and the run of base64 signs that each of its
/// three alignments in a base64 text always holds.
fn forms(marker: &str) -> Vec<String> {
    let bytes = marker.as_bytes();
    let mut forms = vec![marker.to_owned()];
    for shift in 0..3 {
        let mut text = vec![0u8; shift];
        text.extend_from_slice(bytes);
        let encoded = base64(&text);
        let first = (8 * shift).div_ceil(6);
        let end = (8 * (shift + bytes.len())) / 6;
        forms.push(encoded[first..end].to_owned());
    }
    forms
}

/// Whether the row's text holds the marker in one of its forms.
pub fn holds(row: &Row, marker: &str) -> bool {
    forms(marker)
        .iter()
        .any(|form| row.1.contains(form.as_str()))
}

/// `agent_deleted` (A14.4; A7.3): before the delete the first workflow was stored and rows of the family named it;
/// after the delete no row of the family names it; and every row of the family that named the second workflow before
/// the delete, of which there was at least one, stands after it, read by the same keyed read.
pub fn deleted(delete: &Delete) -> bool {
    let kept = naming(&delete.before, &delete.second);
    delete.stored
        && !naming(&delete.before, &delete.first).is_empty()
        && naming(&delete.after, &delete.first).is_empty()
        && !kept.is_empty()
        && kept.iter().all(|row| delete.after.contains(row))
}

/// `agent_scrub` (A14.4, in the form of the coordinator's ruling of 2026-11-21 on the learning run's U3.1b): each of
/// the three markers stood in a row of the first workflow before the delete call; the store called the delete guard
/// once during that call, for the first workflow, and the rows that it read under that workflow, inside the delete's
/// own transaction and before any removal, held each marker; the call committed exactly one transaction, whose report
/// raises the revision of the family; and after the call no row of the family that the keyed read hands out holds a
/// marker. So a removal by another path, before the call or by a transaction of its own, makes the check miss. It
/// reads rows, not bytes.
pub fn scrub(delete: &Delete) -> bool {
    let first = naming(&delete.before, &delete.first);
    let seeded = MARKERS
        .iter()
        .all(|marker| first.iter().any(|row| holds(row, marker)));
    let guarded = match delete.guarded.as_slice() {
        [(id, rows)] => {
            *id == delete.first
                && MARKERS
                    .iter()
                    .all(|marker| rows.iter().any(|row| holds(row, marker)))
        }
        _ => false,
    };
    let gone = !MARKERS
        .iter()
        .any(|marker| delete.after.iter().any(|row| holds(row, marker)));
    seeded && guarded && delete.reports == 1 && delete.family_rose && gone
}

/// The three lines' results for the play's delete.
pub fn outcomes(delete: &Delete) -> [(Check, Outcome); 3] {
    Check::ALL.map(|check| {
        let outcome = match check {
            Check::Deleted => Outcome::holds(deleted(delete)),
            Check::Scrub => Outcome::holds(scrub(delete)),
            Check::DeleteAll => Outcome::NotRun,
        };
        (check, outcome)
    })
}

/// One line `privacy check=<name> result=<ok|miss|not-run>` for each check, in order, each ending with a line feed.
pub fn render(outcomes: &[(Check, Outcome)]) -> String {
    outcomes
        .iter()
        .map(|(check, outcome)| {
            format!(
                "privacy check={} result={}\n",
                check.name(),
                outcome.as_str()
            )
        })
        .collect()
}

/// A key for the store that this process makes from the system's random source.
pub fn process_key() -> Result<DbKey, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| "the system gives no random bytes for a key".to_owned())?;
    let key = DbKey::new(bytes);
    bytes.fill(0);
    std::hint::black_box(&bytes);
    Ok(key)
}

/// A clock that stands at noon of `DAY`, with UTC as local time.
struct Noon;

impl Clock for Noon {
    fn now(&self) -> Instant {
        Instant(i64::from(DAY) * DAY_NS + DAY_NS / 2)
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

/// Ids for the store: a count that fills every byte, so that each id differs from the one before.
struct Count(u8);

impl IdSource for Count {
    fn fill(&mut self, bytes: &mut [u8]) {
        self.0 = self.0.wrapping_add(1);
        bytes.fill(self.0);
    }
}

fn the_family() -> impl Iterator<Item = &'static KindEntry> {
    registry().filter(|entry| entry.family == FAMILY)
}

fn read_family(rows: &dyn RowRead) -> Vec<Row> {
    let mut found = Vec::new();
    for entry in the_family() {
        rows.for_each(entry.name, &mut |row| {
            found.push((entry.name.to_owned(), row.to_owned()));
        });
    }
    found
}

/// The gate's read under `READ_ROLE`: every row of every kind of the family, as `[[<kind>, <row>], ...]`.
struct GateRead;

impl Handler for GateRead {
    fn handle(
        &self,
        _request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let rows = read_family(&cx.rows());
        let json = serde_json::to_string(&rows).map_err(|_| ErrorName::Internal)?;
        RawValue::from_string(json).map_err(|_| ErrorName::Internal)
    }
}

type Guarded = Rc<RefCell<Vec<(WorkflowId, Vec<Row>)>>>;

/// The derived store of the play, its reports and what its delete guard read.
struct Gate {
    store: Store,
    reports: Rc<RefCell<Vec<Report>>>,
    guarded: Guarded,
    sent: u32,
}

fn episode(n: u8, day: u32, app: &str) -> Result<Episode, String> {
    let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::from(n);
    Ok(Episode {
        id: SummaryId::from_random([n; 16]),
        category: SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse(app).map_err(|_| "the app of a summary")?,
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(u64::from(n)).map_err(|_| "the ordinal of a summary")?],
        expires: Instant(i64::MAX),
        start: Instant(start),
        end: Instant(start + 1_000),
        steps: vec![ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse("Pay bill").map_err(|_| "the title of a step")?,
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: None,
            text: None,
        }],
    })
}

impl Gate {
    fn open(data_dir: &Path, key: &DbKey) -> Result<Gate, String> {
        let reports = Rc::new(RefCell::new(Vec::new()));
        let guarded = Guarded::default();
        let seen = Rc::clone(&guarded);
        let sink = Rc::clone(&reports);
        let host = halvo_owner::steps::list()
            .handler(FAMILY, Takes::Role(READ_ROLE), Box::new(GateRead))
            .delete_guard(
                FAMILY,
                Box::new(move |id, rows, _| {
                    seen.borrow_mut().push((id.clone(), read_family(rows)));
                    GuardAnswer::Go
                }),
            )
            .host(Box::new(Noon))
            .on_report(Box::new(move |report: &Report| {
                sink.borrow_mut().push(report.clone());
            }));
        let entries: Vec<&'static KindEntry> = registry().collect();
        let store = Store::open(data_dir, key, &entries, host, &mut Count(0))
            .map_err(|e| format!("the derived store does not open: {e}"))?;
        Ok(Gate {
            store,
            reports,
            guarded,
            sent: 0,
        })
    }

    fn request_id(&mut self) -> RequestId {
        self.sent += 1;
        let mut bytes = [0u8; 16];
        bytes[12..].copy_from_slice(&self.sent.to_be_bytes());
        RequestId::from_random(bytes)
    }

    /// The store's answer to one request of the name with the body, as the owner's reader decodes it.
    fn send(&mut self, name: &str, body: &str) -> Result<String, String> {
        let id = self.request_id();
        let text = format!(
            r#"{{"v":1,"id":"{}","request":"{name}","body":{body}}}"#,
            id.as_str()
        );
        let Ok(Decoded::Raw(raw)) = request::decode(text.as_bytes()) else {
            return Err(format!("a {name} request that the owner's reader refuses"));
        };
        self.store
            .handle(&raw)
            .map(|reply| reply.get().to_owned())
            .map_err(|e| format!("the store refuses a {name} request: {e:?}"))
    }

    fn rows(&mut self) -> Result<Vec<Row>, String> {
        let reply = self.send(READ_ROLE, "{}")?;
        serde_json::from_str(&reply).map_err(|_| "the gate's read gives no list".to_owned())
    }

    fn listed(&mut self) -> Result<Vec<Workflow>, String> {
        let read = InterfaceRead::workflows(&InterfaceRole::claim(), self.request_id(), None);
        match self.store.interface_read(&read) {
            Ok(ReplyResult::Workflows(page)) if page.next.is_none() => Ok(page
                .workflows
                .into_iter()
                .map(|item| item.workflow)
                .collect()),
            _ => Err("the read of the workflows is refused or has a second page".to_owned()),
        }
    }

    /// Two workflows of the category, one for each app, each from three summaries of three days, both approved:
    /// the first workflow's id and the second's.
    fn seed(&mut self) -> Result<(WorkflowId, WorkflowId), String> {
        let mut episodes = Vec::new();
        for (n, app) in [(1, FIRST_APP), (4, SECOND_APP)] {
            for back in 0..3u8 {
                episodes.push(episode(n + back, DAY - u32::from(back), app)?);
            }
        }
        let source = StoreId::parse(&"1".repeat(32)).map_err(|_| "the gate's store id")?;
        let outcome = self
            .store
            .commit_batch(
                &source,
                &episodes,
                Position { seg: 7, idx: 0 },
                &mut Count(100),
            )
            .map_err(|e| format!("the batch is refused: {e}"))?;
        if outcome.proposals.stored != 2 {
            return Err("the step does not propose the two workflows".to_owned());
        }
        let of_app = |workflows: &[Workflow], app: &str| {
            workflows.iter().find_map(|workflow| match workflow {
                Workflow::Full(full) if full.app.as_str() == app => Some(full.id.clone()),
                _ => None,
            })
        };
        let listed = self.listed()?;
        let (Some(first), Some(second)) = (of_app(&listed, FIRST_APP), of_app(&listed, SECOND_APP))
        else {
            return Err("the two workflows are not listed".to_owned());
        };
        for id in [&first, &second] {
            let approve =
                InterfaceDecision::approve(&InterfaceRole::claim(), self.request_id(), id.clone());
            self.store
                .decide(&approve)
                .map_err(|e| format!("the approval is refused: {e}"))?;
        }
        Ok((first, second))
    }

    /// The agent of the workflow as the coordinator and the runner store it: `create_agent`, `set_bindings` with two
    /// slots, `start_run` in the mode compile, and `put_session` of one part whose data is the base64 of `session`.
    fn agent(
        &mut self,
        workflow: &WorkflowId,
        agent: &str,
        run: &str,
        slots: [&str; 2],
        session: &str,
    ) -> Result<(), String> {
        let head = format!(r#""workflow":"{}","agent":"{agent}""#, workflow.as_str());
        let [password, token] = slots;
        for body in [
            format!(r#"{{"op":"create_agent",{head}}}"#),
            format!(
                r#"{{"op":"set_bindings",{head},"slots":[{{"name":"password","value":["{password}"]}},{{"name":"token","value":["{token}"]}}]}}"#
            ),
            format!(r#"{{"op":"start_run",{head},"run":"{run}","mode":"compile"}}"#),
        ] {
            self.send("coordinator_write", &body)?;
        }
        let data = base64(session.as_bytes());
        let put = format!(
            r#"{{"op":"put_session","agent":"{agent}","run":"{run}","origin":"http://utility.test:18090","rev":1,"part":1,"of":1,"data":["{data}"]}}"#
        );
        self.send("runner", &put)?;
        Ok(())
    }

    /// The decision `delete` of the workflow: the store's delete cascade (W-C3.10).
    fn delete(&mut self, id: &WorkflowId) -> Result<(), String> {
        let delete =
            InterfaceDecision::delete(&InterfaceRole::claim(), self.request_id(), id.clone());
        self.store
            .decide(&delete)
            .map_err(|e| format!("the delete is refused: {e}"))
    }

    fn revision(report: Option<&Report>) -> u64 {
        report
            .and_then(|r| r.revisions.iter().find(|(family, _)| *family == FAMILY))
            .map_or(0, |(_, n)| *n)
    }
}

/// The play on a new derived store under `data_dir`, opened with `key`: two workflows with an agent each, the first
/// with the three markers, then the one decision `delete` of the first workflow, with what the store handed out
/// around that call. The store stays under `data_dir`.
pub fn play(data_dir: &Path, key: &DbKey) -> Result<Delete, String> {
    let mut gate = Gate::open(data_dir, key)?;
    let (first, second) = gate.seed()?;
    let [password, token, page] = MARKERS;
    gate.agent(
        &first,
        "40000000000000000000000000000001",
        "50000000000000000000000000000001",
        [password, token],
        &format!(r#"{{"cookies":[{{"name":"ref","value":"{page}"}}]}}"#),
    )?;
    gate.agent(
        &second,
        "40000000000000000000000000000002",
        "50000000000000000000000000000002",
        ["a password of the second", "a token of the second"],
        r#"{"cookies":[{"name":"ref","value":"the second"}]}"#,
    )?;
    let stored = gate.listed()?.iter().any(|w| *w.id() == first);
    let before = gate.rows()?;
    gate.guarded.take();
    let reported = gate.reports.borrow().len();
    gate.delete(&first)?;
    let during: Vec<Report> = gate.reports.borrow()[reported..].to_vec();
    let last_before = reported
        .checked_sub(1)
        .and_then(|at| gate.reports.borrow().get(at).cloned());
    let guarded = gate.guarded.take();
    let after = gate.rows()?;
    Ok(Delete {
        first,
        second,
        stored,
        before,
        after,
        guarded,
        reports: during.len(),
        family_rose: Gate::revision(during.first()) > Gate::revision(last_before.as_ref()),
    })
}
