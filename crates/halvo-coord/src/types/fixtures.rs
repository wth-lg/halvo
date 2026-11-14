//! The page's fixtures (I8.3), written from the Rust types. `facts(state)` gives the facts that
//! each row of a state was written from (I1.3), and `write(dir)` writes the files of I8.3.
//!
//! What a state holds beyond the words of I9.4, and the other choices that I8.3 leaves to this
//! file. The clock stands at 2026-11-03T12:00:00Z (day 20760); `seq` is 1. In each state the
//! random bytes count up from 1: first the workflow id and the agent id of each agent, in the
//! order of the categories of I2.3, then for each agent in that order the ids of its runs, oldest
//! first, the live run last, and then the id of its open item. No effect id or approval id
//! stands in a fixture. Each agent's configuration is the preset of its category (A-A1.3) with
//! `rev` 1 and `playbook` null, or `rev` 2 and `playbook` 1 where a run of a web agent has
//! completed; `limits` are 20, 120 and 0 for a category with a task and 1, 1 and 0 for one with
//! the runtime `none`, so the line `time` says 2 minutes. A workflow with no `schedule` trigger
//! is `manual` with `last` on day 20730. A run started by the person has the slot null unless
//! the table says otherwise. Days are written as `words::day` writes them.
//!
//! | state | agents (category: status line; controls) | facts and item |
//! |---|---|---|
//! | `empty` | none | `note` null |
//! | `ten` | `utility_bill_payment`: `all_set.done`; `run_now`, `switch_off`, `delete` | `active`, `schedule` every 30 days, `last` 20750, so `next` is 20780; slots `start`, `payee`, `instrument`; runs on 20720 `nothing_to_do` and on 20750 `completed` |
//! | | `tenant_rent`, `lease_renewal_or_change`, `tenant_application_review`, `ride_order`, `daily_chart_check`: `not_yet.category`; `switch_off`, `delete` | `approved`, no run |
//! | | `email`: `all_set.done`; `run_now`, `switch_off`, `delete` | `active`; slot `reply_senders`; a run on 20758 `completed` |
//! | | `spreadsheet_fill`: `all_set.nothing`; `run_now`, `switch_off`, `delete` | `active`, `schedule` every 7 days, `last` 20750, so `next` is 20764; its four slots; runs on 20750 `completed` and on 20757 `nothing_to_do`, started by the scheduler for the slot 20757 |
//! | | `shop_order`: `all_set.done`; `run_now`, `switch_off`, `delete` | `active`; its three slots; a run on 20755 `completed` |
//! | | `calendar_cleanup`: `all_set.done`; `run_now`, `switch_off`, `delete` | `active`, `schedule` every day, `last` 20759, so `next` is 20761; runs on 20759 `completed` and on 20760 `completed`, the second with the slot 20760 |
//! | `card` | the agents of `ten` | the card of `utility_bill_payment`, the first agent made (agent id 2), whose two runs are the card's, the newer with one committed `payment` effect whose four kept fields are those of the item of `needs_check` (ruling H-8); the act `open_card` with its id is U3.3's |
//! | `needs_confirm_page` | `email`: `needs_you.confirm`; `stop`, `switch_off`, `delete` | as in `ten`, and a live run in the phase `waiting` with an open `confirm` item of three effects of the class `send`, presenter `page`, `armed` `yes`, no `new_origin` |
//! | `needs_confirm_native` | `utility_bill_payment`: `needs_you.confirm`; `stop`, `switch_off`, `delete` | as in `ten`, and a live run `waiting` with an open `confirm` item of one effect of the class `payment`, presenter `native`, `armed` `no`, its one choice `present` |
//! | `needs_sign_in` | `shop_order`: `needs_you.sign_in`; `stop`, `switch_off`, `delete` | `approved`, no playbook, no ended run, its three slots, a live first run `waiting` with an open `sign_in` item |
//! | `needs_sign_in_bind` | `shop_order`: `needs_you.sign_in`; `stop`, `switch_off`, `delete` | as in `needs_sign_in`, and the open `sign_in` item is its second step, for the origin `https://`, the workflow's site value and the port 443, so that its text names the site value and its choices are `decline` and `allow` |
//! | `needs_link_account` | `email`: `needs_you.link_account`; `stop`, `switch_off`, `delete` | `active`, no ended run, a live first run `waiting` with an open `link_account` item, whose request's origin is `https://`, the workflow's site value and the port 443, so that its field shows the site value |
//! | `needs_stuck` | `utility_bill_payment`: `needs_you.stuck`; `stop`, `switch_off`, `delete` | as in `ten`, and a live run `waiting` with an open `stuck` item |
//! | `needs_check` | `utility_bill_payment`: `needs_you.check`; `switch_off`, `delete` | as in `ten`, and a run on 20759 `effect_unknown` with `unknown` 1, so the hold `check` stands; its `check` item shows the four kept fields of a `payment` |
//! | `needs_setup` | `utility_bill_payment`: `needs_you.setup`; `switch_off`, `delete` | `approved`, no run, slots `start` and `instrument`, so the hold `setup` stands for `payee`, which has one candidate from `records`, the workflow's site value |
//! | `needs_ready` | `email`: `needs_you.ready`; `run_now`, `switch_off`, `delete` | `active`, its slot, no run, so its first run waits for the person |
//! | `needs_show` | `shop_order`: `needs_you.show`; `stop`, `switch_off`, `delete` | `approved`, no playbook, its three slots, a live first run `waiting` with an open `show` item |
//! | `setup_three` | `shop_order`: `needs_you.setup`; `switch_off`, `delete` | `approved`, no slot, no run; `start` and `seller` each with one candidate from `records`, the site value, and `instrument` with none, so `typed` `yes` |
//! | `problem` | `spreadsheet_fill`: `problem.limit`; `run_now`, `switch_off`, `delete` | as in `ten` but the run on 20757 ended `limit` |
//! | | `shop_order`: `problem.failures`; `run_now`, `switch_on`, `delete` | `paused`; runs on 20750 `completed` and on 20757, 20758 and 20759 `failed`; `hold_set` `failures` on 20759 |
//! | `off` | `email`: `off.person`; `switch_on`, `delete` | `paused`; a run on 20758 `completed`; `hold_set` `person` on 20759 |
//! | `working` | `utility_bill_payment`: `working.working`; `stop`, `switch_off`, `delete` | as in `ten`, and a live run in the phase `working` |
//! | `queue` | `utility_bill_payment` as in `working`; `email` and `shop_order`: `waiting_turn.queued`; `switch_off`, `delete` | `email` and `shop_order` as in `ten`, each with a start in the queue |
//! | `no_service` | none | `note` the text of `note.no_service` |
//! | `needs_confirm_mixed` | `email` as in `needs_confirm_page` | its `confirm` item asks for one effect of the class `send` and one of the class `delete`, so it takes the mixed title and controls, and the fields of each effect follow its action line (ruling RD-10 (b), IG-4) |
//! | `needs_confirm_changed` | `email` as in `needs_confirm_page` | its `confirm` item asks for one effect of the class `send` whose newest released decision in the run is `allow_once`, so the line `confirm.changed` stands above its values (ruling RD-10 (b), IG-25) |
//!
//! Choices of this file beyond the table: `reply-next-same.json` is the snapshot with `seq` 1,
//! empty lists and the note null. The `values` of the argument `choice` of `agents_answer` are
//! the ten choices of I3.1 (`commands::LIST`). An item that is no confirmation has `armed` `no`;
//! a page confirmation has `armed` `yes`, because its card is on screen, and a native one `no`,
//! because its dialog is shown only after `agents_present`. A `setup` item lists both choices,
//! `none` and `choose`. The title of an item that is no confirmation names the agent through
//! `{name}`. The sentences `line.run.<end>` have no placeholder. Every value of a field and of a
//! candidate holds only letters, digits, spaces and a point. The entries of `agents_next` follow
//! the state-sync verdict `sync=get` of I8.2.

use super::names::names;
use super::{commands, Class, Doing, Live, OwnItem, OwnItemKind, Raised, RunItem, RunItemKind};
use super::{
    hex, AgentId, BoundFrom, Candidate, CandidateFrom, Card, CardEffect, CardLine, CardRun,
    Category, Cause, Choice, ChoiceText, ClosedReply, ConfigView, Control, ControlText, ErrorReply,
    Event, Facts, Field, FieldFrom, FullWorkflow, HoldSetCause, Item, ItemId, ItemKind, Kept,
    LineKey, LivePhase, Name, OkReply, OpenItem, Outcome, Param, ParamEntry, PastRun, Permission,
    Presenter, Row, RunId, Runtime, Snapshot, StatusLine, Tier, Trigger, What, WorkflowId,
    WorkflowState, WorkflowTrigger, YesNo,
};
use crate::mem::Config;
use crate::words;
use serde::Serialize;
use std::num::NonZeroU32;
use std::path::Path;

const TODAY: u32 = 20_760;
const DAY_NS: i64 = 86_400_000_000_000;
const NOON_NS: i64 = 43_200_000_000_000;

names! {
    /// The states of I9.4 and ruling RD-10 (b), each a file `state-<state>.json`.
    State {
        Empty = "empty",
        Ten = "ten",
        NeedsConfirmPage = "needs_confirm_page",
        NeedsConfirmNative = "needs_confirm_native",
        NeedsSignIn = "needs_sign_in",
        NeedsLinkAccount = "needs_link_account",
        NeedsStuck = "needs_stuck",
        NeedsCheck = "needs_check",
        NeedsSetup = "needs_setup",
        NeedsReady = "needs_ready",
        NeedsShow = "needs_show",
        SetupThree = "setup_three",
        Card = "card",
        Problem = "problem",
        Off = "off",
        Working = "working",
        Queue = "queue",
        NoService = "no_service",
        NeedsSignInBind = "needs_sign_in_bind",
        NeedsConfirmMixed = "needs_confirm_mixed",
        NeedsConfirmChanged = "needs_confirm_changed",
    }
}

#[derive(Clone, Copy)]
struct Past {
    day: u32,
    end: Outcome,
    trigger: Trigger,
    slot: Option<u32>,
    unknown: u32,
    committed: Option<(Class, &'static [Bound])>,
}

fn past(day: u32, end: Outcome) -> Past {
    Past {
        day,
        end,
        trigger: Trigger::Person,
        slot: None,
        unknown: 0,
        committed: None,
    }
}

struct Spec {
    category: Category,
    state: WorkflowState,
    trigger: WorkflowTrigger,
    period: Option<u32>,
    last: u32,
    slots: Vec<Param>,
    playbook: bool,
    runs: Vec<Past>,
    hold: Option<(HoldSetCause, u32)>,
    live: Option<LivePhase>,
    queued: bool,
    item: Option<ItemKind>,
    line: StatusLine,
    controls: Vec<Control>,
    next: Option<u32>,
}

fn params(category: Category) -> Vec<Param> {
    match category {
        Category::UtilityBillPayment => vec![Param::Start, Param::Payee, Param::Instrument],
        Category::Email => vec![Param::ReplySenders],
        Category::SpreadsheetFill => {
            vec![Param::Workbook, Param::Sheet, Param::Range, Param::Source]
        }
        Category::ShopOrder => vec![Param::Start, Param::Seller, Param::Instrument],
        _ => vec![],
    }
}

fn spec(category: Category, line: StatusLine) -> Spec {
    Spec {
        category,
        state: WorkflowState::Active,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: TODAY - 30,
        slots: params(category),
        playbook: false,
        runs: vec![],
        hold: None,
        live: None,
        queued: false,
        item: None,
        line,
        controls: vec![Control::RunNow, Control::SwitchOff, Control::Delete],
        next: None,
    }
}

const LIVE: [Control; 3] = [Control::Stop, Control::SwitchOff, Control::Delete];
const HELD: [Control; 2] = [Control::SwitchOff, Control::Delete];

fn utility(line: StatusLine) -> Spec {
    Spec {
        trigger: WorkflowTrigger::Schedule,
        period: Some(30),
        last: 20_750,
        playbook: true,
        runs: vec![
            past(20_720, Outcome::NothingToDo),
            Past {
                committed: Some((Class::Payment, &PAYMENT)),
                ..past(20_750, Outcome::Completed)
            },
        ],
        next: Some(20_780),
        ..spec(Category::UtilityBillPayment, line)
    }
}

fn utility_waiting(kind: ItemKind) -> Spec {
    Spec {
        live: Some(LivePhase::Waiting),
        item: Some(kind),
        controls: LIVE.to_vec(),
        ..utility(StatusLine::NeedsYou(kind))
    }
}

fn fresh(category: Category, line: StatusLine) -> Spec {
    let state = if category == Category::Email {
        WorkflowState::Active
    } else {
        WorkflowState::Approved
    };
    let base = if category == Category::UtilityBillPayment {
        utility(line)
    } else {
        spec(category, line)
    };
    Spec {
        state,
        playbook: false,
        runs: vec![],
        ..base
    }
}

fn first_run_waiting(category: Category, kind: ItemKind) -> Spec {
    Spec {
        live: Some(LivePhase::Waiting),
        item: Some(kind),
        controls: LIVE.to_vec(),
        ..fresh(category, StatusLine::NeedsYou(kind))
    }
}

fn not_yet(category: Category) -> Spec {
    Spec {
        state: WorkflowState::Approved,
        controls: HELD.to_vec(),
        ..spec(category, StatusLine::NotYet)
    }
}

fn email(line: StatusLine) -> Spec {
    Spec {
        runs: vec![past(20_758, Outcome::Completed)],
        ..spec(Category::Email, line)
    }
}

fn sheet(last_end: Outcome, line: StatusLine) -> Spec {
    Spec {
        trigger: WorkflowTrigger::Schedule,
        period: Some(7),
        last: 20_750,
        runs: vec![
            past(20_750, Outcome::Completed),
            Past {
                trigger: Trigger::Schedule,
                slot: Some(20_757),
                ..past(20_757, last_end)
            },
        ],
        next: Some(20_764),
        ..spec(Category::SpreadsheetFill, line)
    }
}

fn shop(line: StatusLine) -> Spec {
    Spec {
        playbook: true,
        runs: vec![past(20_755, Outcome::Completed)],
        ..spec(Category::ShopOrder, line)
    }
}

fn calendar() -> Spec {
    Spec {
        trigger: WorkflowTrigger::Schedule,
        period: Some(1),
        last: 20_759,
        runs: vec![
            past(20_759, Outcome::Completed),
            Past {
                slot: Some(20_760),
                ..past(20_760, Outcome::Completed)
            },
        ],
        next: Some(20_761),
        ..spec(Category::CalendarCleanup, StatusLine::Done)
    }
}

fn ten() -> Vec<Spec> {
    vec![
        utility(StatusLine::Done),
        not_yet(Category::TenantRent),
        not_yet(Category::LeaseRenewalOrChange),
        not_yet(Category::TenantApplicationReview),
        email(StatusLine::Done),
        sheet(Outcome::NothingToDo, StatusLine::Nothing),
        shop(StatusLine::Done),
        calendar(),
        not_yet(Category::RideOrder),
        not_yet(Category::DailyChartCheck),
    ]
}

fn specs(state: State) -> Vec<Spec> {
    match state {
        State::Empty | State::NoService => vec![],
        State::Ten | State::Card => ten(),
        State::NeedsConfirmPage | State::NeedsConfirmMixed | State::NeedsConfirmChanged => {
            vec![Spec {
                live: Some(LivePhase::Waiting),
                item: Some(ItemKind::Confirm),
                controls: LIVE.to_vec(),
                ..email(StatusLine::NeedsYou(ItemKind::Confirm))
            }]
        }
        State::NeedsConfirmNative => vec![utility_waiting(ItemKind::Confirm)],
        State::NeedsSignIn | State::NeedsSignInBind => {
            vec![first_run_waiting(Category::ShopOrder, ItemKind::SignIn)]
        }
        State::NeedsLinkAccount => {
            vec![first_run_waiting(Category::Email, ItemKind::LinkAccount)]
        }
        State::NeedsStuck => vec![utility_waiting(ItemKind::Stuck)],
        State::NeedsCheck => {
            let mut s = utility(StatusLine::NeedsYou(ItemKind::Check));
            s.runs.push(Past {
                unknown: 1,
                ..past(20_759, Outcome::EffectUnknown)
            });
            s.item = Some(ItemKind::Check);
            s.controls = HELD.to_vec();
            vec![s]
        }
        State::NeedsSetup => vec![Spec {
            slots: vec![Param::Start, Param::Instrument],
            item: Some(ItemKind::Setup),
            controls: HELD.to_vec(),
            ..fresh(
                Category::UtilityBillPayment,
                StatusLine::NeedsYou(ItemKind::Setup),
            )
        }],
        State::NeedsReady => vec![Spec {
            item: Some(ItemKind::Ready),
            ..fresh(Category::Email, StatusLine::NeedsYou(ItemKind::Ready))
        }],
        State::NeedsShow => vec![first_run_waiting(Category::ShopOrder, ItemKind::Show)],
        State::SetupThree => vec![Spec {
            slots: vec![],
            item: Some(ItemKind::Setup),
            controls: HELD.to_vec(),
            ..fresh(Category::ShopOrder, StatusLine::NeedsYou(ItemKind::Setup))
        }],
        State::Problem => vec![
            sheet(Outcome::Limit, StatusLine::Problem(super::Failure::Limit)),
            Spec {
                state: WorkflowState::Paused,
                runs: vec![
                    past(20_750, Outcome::Completed),
                    past(20_757, Outcome::Failed),
                    past(20_758, Outcome::Failed),
                    past(20_759, Outcome::Failed),
                ],
                hold: Some((HoldSetCause::Failures, 20_759)),
                controls: vec![Control::RunNow, Control::SwitchOn, Control::Delete],
                ..shop(StatusLine::Failures)
            },
        ],
        State::Off => vec![Spec {
            state: WorkflowState::Paused,
            hold: Some((HoldSetCause::Person, 20_759)),
            controls: vec![Control::SwitchOn, Control::Delete],
            ..email(StatusLine::Off)
        }],
        State::Working => vec![Spec {
            live: Some(LivePhase::Working),
            controls: LIVE.to_vec(),
            ..utility(StatusLine::Working)
        }],
        State::Queue => vec![
            Spec {
                live: Some(LivePhase::Working),
                controls: LIVE.to_vec(),
                ..utility(StatusLine::Working)
            },
            Spec {
                queued: true,
                controls: HELD.to_vec(),
                ..email(StatusLine::Queued)
            },
            Spec {
                queued: true,
                controls: HELD.to_vec(),
                ..shop(StatusLine::Queued)
            },
        ],
    }
}

struct Built {
    spec: Spec,
    facts: Facts<Config>,
}

fn number(category: Category) -> usize {
    Category::ALL
        .iter()
        .position(|c| *c == category)
        .expect("a seed category")
        + 1
}

fn config(spec: &Spec, workflow: &WorkflowId, agent: &AgentId) -> Config {
    let (runtime, autonomy, untrusted, external, permissions, limits) = match spec.category {
        Category::UtilityBillPayment | Category::ShopOrder => (
            "web",
            "draft_and_confirm",
            "yes",
            "yes",
            "",
            "20,\"wall_s\":120",
        ),
        Category::Email => (
            "graph_mail",
            "draft_and_confirm",
            "yes",
            "yes",
            "",
            "20,\"wall_s\":120",
        ),
        Category::CalendarCleanup => (
            "graph_calendar",
            "draft_and_confirm",
            "yes",
            "yes",
            "",
            "20,\"wall_s\":120",
        ),
        Category::SpreadsheetFill => (
            "file",
            "draft_and_confirm",
            "no",
            "no",
            "\"user_files\"",
            "20,\"wall_s\":120",
        ),
        _ => ("none", "observe", "yes", "no", "", "1,\"wall_s\":1"),
    };
    let (rev, playbook) = if spec.playbook { (2, "1") } else { (1, "null") };
    let record = format!(
        "{{\"v\":1,\"workflow\":\"{workflow}\",\"agent\":\"{agent}\",\"rev\":{rev},\"category\":\"{}\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"{autonomy}\",\"allow\":[],\"caps\":{{\"untrusted\":\"{untrusted}\",\"private\":\"yes\",\"external\":\"{external}\"}},\"limits\":{{\"steps\":{limits},\"model_tokens\":0}},\"budget\":null,\"permissions\":[{permissions}],\"runtime\":\"{runtime}\",\"playbook\":{playbook}}}",
        spec.category.name()
    );
    Config::parse(&record).expect("a record of A-A2.1")
}

fn own(open: OpenItem) -> OwnItem {
    let kind = match open.kind {
        ItemKind::Check => OwnItemKind::Check,
        ItemKind::Setup => OwnItemKind::Setup,
        ItemKind::Ready => OwnItemKind::Ready,
        _ => panic!("an item that a run raises, with no live run"),
    };
    OwnItem {
        item: open.item,
        kind,
    }
}

fn of_run(open: OpenItem) -> RunItem {
    let kind = match open.kind {
        ItemKind::Confirm => RunItemKind::Confirm,
        ItemKind::SignIn => RunItemKind::SignIn,
        ItemKind::LinkAccount => RunItemKind::LinkAccount,
        ItemKind::Stuck => RunItemKind::Stuck,
        ItemKind::Check => RunItemKind::Check,
        ItemKind::Show => RunItemKind::Show,
        _ => panic!("an item that no run raises, in a waiting run"),
    };
    RunItem {
        item: open.item,
        kind,
    }
}

fn doing(spec: &Spec, open: Option<OpenItem>) -> Doing {
    match (spec.live, spec.queued, open) {
        (None, false, open) => Doing::Idle(open.map(own)),
        (None, true, open) => Doing::Queued(open.map(own)),
        (Some(LivePhase::Starting), false, None) => Doing::Live(Live::Starting),
        (Some(LivePhase::Working), false, None) => Doing::Live(Live::Working),
        (Some(LivePhase::Waiting), false, Some(open)) => Doing::Live(Live::Waiting(of_run(open))),
        (Some(LivePhase::Stopping), false, None) => Doing::Live(Live::Stopping),
        _ => panic!("a spec of facts that I5 rules out"),
    }
}

fn raised(kind: ItemKind, live: Option<RunId>) -> Raised {
    let run = || live.clone().expect("a live run raised the item");
    match kind {
        ItemKind::Confirm => Raised::Confirm(run()),
        ItemKind::SignIn => Raised::SignIn(run()),
        ItemKind::LinkAccount => Raised::LinkAccount(run()),
        ItemKind::Stuck => Raised::Stuck(run()),
        ItemKind::Show => Raised::Show(run()),
        ItemKind::Check => Raised::Check(None),
        ItemKind::Setup => Raised::Setup,
        ItemKind::Ready => Raised::Ready,
    }
}

fn build(state: State) -> Vec<Built> {
    let specs = specs(state);
    let mut count: u128 = 0;
    let mut next = || {
        count += 1;
        hex(count.to_be_bytes())
    };
    let pairs: Vec<(WorkflowId, AgentId)> = specs
        .iter()
        .map(|_| {
            let w = WorkflowId::parse(&next()).expect("an id");
            let a = AgentId::parse(&next()).expect("an id");
            (w, a)
        })
        .collect();
    let mut built = Vec::new();
    for (spec, (workflow, agent)) in specs.into_iter().zip(pairs) {
        let runs: Vec<RunId> = spec
            .runs
            .iter()
            .map(|_| RunId::parse(&next()).expect("an id"))
            .collect();
        let live = spec.live.map(|_| RunId::parse(&next()).expect("an id"));
        let item = spec.item.map(|_| ItemId::parse(&next()).expect("an id"));
        let mut events = Vec::new();
        let mut push = |day: u32, what: What| {
            events.push(Event {
                workflow: workflow.clone(),
                agent: agent.clone(),
                seq: NonZeroU32::MIN.saturating_add(events.len() as u32),
                day,
                at: i64::from(day) * DAY_NS + NOON_NS,
                what,
            });
        };
        for (p, run) in spec.runs.iter().zip(&runs) {
            push(
                p.day,
                What::RunStarted {
                    run: run.clone(),
                    trigger: p.trigger,
                    slot: p.slot,
                },
            );
            push(
                p.day,
                What::RunEnded {
                    run: run.clone(),
                    cause: p.end.into(),
                },
            );
        }
        if let Some((cause, day)) = spec.hold {
            push(day, What::HoldSet { cause });
        }
        if let Some(run) = &live {
            push(
                TODAY,
                What::RunStarted {
                    run: run.clone(),
                    trigger: Trigger::Person,
                    slot: None,
                },
            );
        }
        if let (Some(item), Some(kind)) = (&item, spec.item) {
            push(
                TODAY,
                What::ItemRaised {
                    item: item.clone(),
                    raised: raised(kind, live.clone()),
                },
            );
        }
        let mut history: Vec<PastRun> = spec
            .runs
            .iter()
            .zip(&runs)
            .map(|(p, run)| PastRun {
                run: run.clone(),
                day: p.day,
                outcome: p.end,
                unknown: p.unknown,
            })
            .collect();
        history.reverse();
        history.truncate(5);
        let doing = doing(
            &spec,
            item.zip(spec.item)
                .map(|(item, kind)| OpenItem { item, kind }),
        );
        let facts = Facts {
            config: config(&spec, &workflow, &agent),
            workflow: FullWorkflow {
                id: workflow,
                state: spec.state,
                category: spec.category,
                site: Some(format!("site{}.example", number(spec.category))),
                trigger: spec.trigger,
                period: spec
                    .period
                    .map(|p| NonZeroU32::new(p).expect("a period of 1 day or more")),
                last: spec.last,
                agent: Some(agent.clone()),
            },
            agent,
            slots: spec
                .slots
                .iter()
                .map(|p| Name::parse(p.name()).expect("a name"))
                .collect(),
            runs: history,
            events,
            store_ended: !matches!(doing, Doing::Live(_)),
            today: TODAY,
            doing,
        };
        built.push(Built { spec, facts });
    }
    built
}

/// The facts of each agent of a state, in the order of the categories of I2.3, from which the
/// rows, the item and the card of `state-<state>.json` were written (I1.3).
pub fn facts(state: State) -> Vec<Facts<Config>> {
    build(state).into_iter().map(|b| b.facts).collect()
}

fn t(key: &str) -> String {
    words::text(key, &[]).expect("a key of the table")
}

fn tp(key: &str, pairs: &[(&str, &str)]) -> String {
    words::text(key, pairs).expect("a key of the table with its placeholders")
}

fn name(category: Category) -> String {
    tp(
        "name.with_site",
        &[
            ("category", &t(&format!("category.{}", category.name()))),
            ("site", &format!("Site{}", number(category))),
        ],
    )
}

fn row(b: &Built) -> Row {
    let status = b.spec.line.status();
    let name = name(b.spec.category);
    Row {
        agent: b.facts.agent.clone(),
        workflow: b.facts.workflow.id.clone(),
        say: tp(&format!("say.{}", status.name()), &[("name", &name)]),
        name,
        status,
        word: t(&format!("status.{}", status.name())),
        line: t(&b.spec.line.key()),
        level: t(&format!("line.level.{}", b.facts.config.reasoning().name())),
        next: b.spec.next.map(words::day),
        controls: b
            .spec
            .controls
            .iter()
            .map(|c| ControlText {
                control: *c,
                text: t(&format!("control.{}", c.name())),
            })
            .collect(),
    }
}

fn from_of(from: BoundFrom) -> FieldFrom {
    match from {
        BoundFrom::Slot => FieldFrom::You,
        BoundFrom::Extract => FieldFrom::Site,
        BoundFrom::Model => FieldFrom::Agent,
        BoundFrom::Const => FieldFrom::Halvo,
    }
}

type Bound = (&'static str, BoundFrom, &'static str);

const PAYMENT: [Bound; 4] = [
    ("payee", BoundFrom::Slot, "City Power"),
    ("amount", BoundFrom::Extract, "42.10 USD"),
    ("instrument", BoundFrom::Slot, "Card ending 4242"),
    ("origin", BoundFrom::Const, "site1.example"),
];

const SEND: [[Bound; 3]; 3] = [
    [
        ("to", BoundFrom::Extract, "Ann Lee"),
        ("subject", BoundFrom::Extract, "Rent receipt for October"),
        (
            "body",
            BoundFrom::Model,
            "Thank you. Your rent for October came in.",
        ),
    ],
    [
        ("to", BoundFrom::Extract, "Ben Ola"),
        ("subject", BoundFrom::Extract, "Viewing on Friday"),
        (
            "body",
            BoundFrom::Model,
            "Friday at 10 works for the viewing.",
        ),
    ],
    [
        ("to", BoundFrom::Extract, "Cara Diaz"),
        ("subject", BoundFrom::Extract, "Repair date"),
        ("body", BoundFrom::Model, "The repair is booked for Monday."),
    ],
];

const MIXED_SEND: [Bound; 3] = [
    ("to", BoundFrom::Extract, "Ann Lee"),
    ("subject", BoundFrom::Extract, "Rent receipt for October"),
    (
        "body",
        BoundFrom::Model,
        "Thank you. Your rent for October came in.",
    ),
];

const MIXED_DELETE: [Bound; 1] = [("items", BoundFrom::Extract, "Draft to Ann Lee from October")];

const CHANGED_SEND: [Bound; 3] = [
    ("to", BoundFrom::Extract, "Ben Ola"),
    ("subject", BoundFrom::Extract, "Viewing on Friday"),
    (
        "body",
        BoundFrom::Model,
        "Friday at 11 works for the viewing.",
    ),
];

fn fields(class: Class, bound: &[Bound]) -> Vec<Field> {
    bound
        .iter()
        .map(|(field, from, value)| {
            let from = from_of(*from);
            Field {
                label: t(&format!("field.{}.{field}", class.name())),
                value: vec![(*value).to_owned()],
                from,
                from_text: t(&format!("from.{}", from.name())),
            }
        })
        .collect()
}

fn halvo_line(key: &str) -> Field {
    Field {
        label: t(key),
        value: vec![],
        from: FieldFrom::Halvo,
        from_text: t(&format!("from.{}", FieldFrom::Halvo.name())),
    }
}

fn mixed(id: ItemId, agent: AgentId) -> Item {
    let effects: [(Class, &[Bound]); 2] =
        [(Class::Send, &MIXED_SEND), (Class::Delete, &MIXED_DELETE)];
    let count = [("count", "2")];
    Item::confirm(
        id,
        agent,
        Presenter::Page,
        YesNo::Yes,
        tp("confirm.mixed.title", &count),
        effects
            .iter()
            .flat_map(|(class, bound)| {
                let mut f = vec![halvo_line(&format!("confirm.{}.action", class.name()))];
                f.extend(fields(*class, bound));
                f
            })
            .collect(),
        vec![
            ChoiceText {
                choice: Choice::Reject,
                text: tp("confirm.mixed.reject", &count),
                safe: YesNo::Yes,
            },
            ChoiceText {
                choice: Choice::Allow,
                text: tp("confirm.mixed.allow", &count),
                safe: YesNo::No,
            },
        ],
    )
}

fn choice(kind: ItemKind, choice: Choice, safe: bool) -> ChoiceText {
    ChoiceText {
        choice,
        text: t(&format!("choice.{}.{}", kind.name(), choice.name())),
        safe: safe.into(),
    }
}

fn two_choices(kind: ItemKind) -> Vec<ChoiceText> {
    let (safe, other) = match kind {
        ItemKind::Check => (Choice::TryAgain, Choice::WentThrough),
        ItemKind::Setup => (Choice::None, Choice::Choose),
        ItemKind::Ready => (Choice::Skip, Choice::Start),
        _ => (Choice::Decline, Choice::Done),
    };
    vec![choice(kind, safe, true), choice(kind, other, false)]
}

fn param(param: Param, candidate: Option<&str>) -> ParamEntry {
    ParamEntry {
        name: param,
        label: t(&format!("param.{}", param.name())),
        candidates: candidate
            .map(|value| Candidate {
                n: 1,
                value: vec![value.to_owned()],
                from: CandidateFrom::Records,
            })
            .into_iter()
            .collect(),
        typed: candidate.is_none().into(),
        full: if candidate.is_none() {
            t("param_typed.full")
        } else {
            String::new()
        },
        too_long: if candidate.is_none() {
            t("param_typed.too_long")
        } else {
            String::new()
        },
    }
}

fn confirm(state: State, id: ItemId, agent: AgentId) -> Item {
    let (class, presenter, bound): (Class, Presenter, Vec<&[Bound]>) = match state {
        State::NeedsConfirmMixed => return mixed(id, agent),
        State::NeedsConfirmNative => (Class::Payment, Presenter::Native, vec![&PAYMENT[..]]),
        State::NeedsConfirmChanged => (Class::Send, Presenter::Page, vec![&CHANGED_SEND[..]]),
        _ => (
            Class::Send,
            Presenter::Page,
            SEND.iter().map(|e| &e[..]).collect(),
        ),
    };
    let changed = (state == State::NeedsConfirmChanged).then(|| halvo_line("confirm.changed"));
    let count = bound.len().to_string();
    let entry = |c: Choice, key: &str, safe: bool| ChoiceText {
        choice: c,
        text: if bound.len() > 1 {
            tp(
                &format!("confirm.{}.{key}_many", class.name()),
                &[("count", &count)],
            )
        } else {
            t(&format!("confirm.{}.{key}", class.name()))
        },
        safe: safe.into(),
    };
    let choices = if presenter == Presenter::Native {
        vec![ChoiceText {
            choice: Choice::Present,
            text: t("confirm.present"),
            safe: YesNo::Yes,
        }]
    } else {
        vec![
            entry(Choice::Reject, "reject", true),
            entry(Choice::Allow, "allow", false),
        ]
    };
    Item::confirm(
        id,
        agent,
        presenter,
        (presenter == Presenter::Page).into(),
        t(&format!("confirm.{}.title", class.name())),
        changed
            .into_iter()
            .chain(bound.iter().flat_map(|e| fields(class, e)))
            .collect(),
        choices,
    )
}

fn item(state: State, b: &Built) -> Option<Item> {
    let open = b.facts.doing.item()?;
    let kind = open.kind;
    let name = name(b.spec.category);
    let (id, agent) = (open.item.clone(), b.facts.agent.clone());
    let plain = || {
        (
            tp(&format!("item.{}.title", kind.name()), &[("name", &name)]),
            t(&format!("item.{}.text", kind.name())),
            two_choices(kind),
        )
    };
    let item = match kind {
        ItemKind::Confirm => confirm(state, id, agent),
        ItemKind::Check => {
            let (title, text, choices) = plain();
            let kept = fields(Class::Payment, &PAYMENT);
            Item::check(id, agent, title, text, kept, choices)
        }
        ItemKind::Setup => {
            let (title, text, choices) = plain();
            let site = b.facts.workflow.site.clone().expect("a site");
            let asked = params(b.spec.category)
                .into_iter()
                .filter(|p| !b.spec.slots.contains(p))
                .map(|p| match p {
                    Param::Start | Param::Payee | Param::Seller => param(p, Some(&site)),
                    _ => param(p, None),
                })
                .collect();
            Item::setup(id, agent, title, text, None, asked, choices)
        }
        ItemKind::SignIn if state == State::NeedsSignInBind => {
            let (title, _, _) = plain();
            let site = b.facts.workflow.site.clone().expect("a site");
            let text = tp("item.sign_in.bind", &[("origin", &site)]);
            let choices = vec![
                ChoiceText {
                    choice: Choice::Decline,
                    text: t("choice.sign_in.bind_decline"),
                    safe: YesNo::Yes,
                },
                ChoiceText {
                    choice: Choice::Allow,
                    text: t("choice.sign_in.bind_allow"),
                    safe: YesNo::No,
                },
            ];
            Item::sign_in(id, agent, title, text, choices)
        }
        ItemKind::SignIn => {
            let (title, text, choices) = plain();
            Item::sign_in(id, agent, title, text, choices)
        }
        ItemKind::LinkAccount => {
            let (title, text, choices) = plain();
            let site = Field {
                label: t("item.link_account.origin"),
                value: vec![b.facts.workflow.site.clone().expect("a site")],
                from: FieldFrom::Halvo,
                from_text: t("from.halvo"),
            };
            Item::link_account(id, agent, title, text, site, choices)
        }
        ItemKind::Stuck => {
            let (title, text, choices) = plain();
            Item::stuck(id, agent, title, text, choices)
        }
        ItemKind::Ready => {
            let (title, text, choices) = plain();
            Item::ready(id, agent, title, text, choices)
        }
        ItemKind::Show => {
            let (title, text, choices) = plain();
            Item::show(id, agent, title, text, choices)
        }
    };
    Some(item.said_by(&name).expect("the words table holds say.item"))
}

fn committed(class: Class, bound: &[Bound]) -> CardEffect {
    let kept: Vec<Kept> = bound
        .iter()
        .map(|(field, _, value)| Kept {
            name: Name::parse(field).expect("a field name"),
            value: vec![if *field == "origin" {
                format!("https://{value}:443")
            } else {
                (*value).to_owned()
            }],
        })
        .collect();
    CardEffect::committed(class, &kept).expect("the kept fields of the class")
}

fn card(b: &Built) -> Card {
    let c = &b.facts.config;
    let mut lines = vec![CardLine {
        key: LineKey::Level,
        text: t(&format!("line.level.{}", c.reasoning().name())),
    }];
    if c.tier() == Tier::OnDevice {
        lines.push(CardLine {
            key: LineKey::RunsOn,
            text: t("line.runs_on.on_device"),
        });
    }
    lines.push(CardLine {
        key: LineKey::Asks,
        text: t(&format!("line.asks.{}", c.autonomy().name())),
    });
    if c.runtime() != Runtime::None {
        let minutes = c.wall_s().div_ceil(60).to_string();
        lines.push(CardLine {
            key: LineKey::Time,
            text: tp("line.time", &[("minutes", &minutes)]),
        });
    }
    if c.permissions().contains(&Permission::UserFiles) {
        lines.push(CardLine {
            key: LineKey::Files,
            text: t("line.files"),
        });
    }
    Card {
        agent: b.facts.agent.clone(),
        name: name(b.spec.category),
        lines,
        runs: b
            .spec
            .runs
            .iter()
            .rev()
            .take(5)
            .map(|p| CardRun {
                day: words::day(p.day),
                text: t(&format!("line.run.{}", super::RunEnd::from(p.end).name())),
                effects: p
                    .committed
                    .map(|(class, bound)| committed(class, bound))
                    .into_iter()
                    .collect(),
            })
            .collect(),
    }
}

#[derive(Serialize)]
struct StateFile {
    snapshot: Snapshot,
    card: Option<Card>,
}

fn state_file(state: State) -> StateFile {
    let built = build(state);
    let mut order: Vec<(Row, Option<Item>)> =
        built.iter().map(|b| (row(b), item(state, b))).collect();
    order.sort_by(|(a, _), (b, _)| {
        (a.status, a.name.as_bytes(), &a.agent).cmp(&(b.status, b.name.as_bytes(), &b.agent))
    });
    let (rows, items): (Vec<Row>, Vec<Option<Item>>) = order.into_iter().unzip();
    let note = (state == State::NoService).then(|| t("note.no_service"));
    let card = (state == State::Card).then(|| card(&built[0]));
    StateFile {
        snapshot: Snapshot {
            seq: 1,
            rows,
            items: items.into_iter().flatten().collect(),
            note,
        },
        card,
    }
}

fn line<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string(value).expect("a reply serializes");
    text.push('\n');
    text
}

/// Writes the files of I8.3 into `dir`, which it makes if needed, and returns their count;
/// a second call writes the same bytes.
pub fn write(dir: &Path) -> std::io::Result<u32> {
    std::fs::create_dir_all(dir)?;
    let mut files: Vec<(String, String)> = vec![
        ("commands.json".into(), commands::json()),
        ("words.json".into(), words::export()),
    ];
    for state in State::ALL {
        files.push((
            format!("state-{}.json", state.name()),
            line(&state_file(*state)),
        ));
    }
    files.push(("reply-ok.json".into(), line(&OkReply)));
    files.push((
        "reply-closed.json".into(),
        line(&ClosedReply { closed: YesNo::Yes }),
    ));
    files.push((
        "reply-next-same.json".into(),
        line(&Snapshot {
            seq: 1,
            rows: vec![],
            items: vec![],
            note: None,
        }),
    ));
    for cause in Cause::ALL {
        files.push((
            format!("reply-error-{}.json", cause.name()),
            line(&ErrorReply { error: *cause }),
        ));
    }
    for (name, text) in &files {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(files.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::{state_file, FieldFrom, State};

    #[test]
    fn the_newer_run_of_the_card_holds_one_committed_payment_with_its_four_kept_fields() {
        let card = state_file(State::Card)
            .card
            .expect("the state card has a card");
        assert_eq!(card.runs.len(), 2);
        let newer = &card.runs[0];
        assert_eq!(
            (newer.day.as_str(), newer.text.as_str()),
            ("24 October 2026", "It finished the task.")
        );
        assert_eq!(newer.effects.len(), 1);
        let paid = &newer.effects[0];
        assert_eq!(
            (paid.key().as_str(), paid.text()),
            ("run.effect.payment", "It made this payment.")
        );
        let fields: Vec<(&str, &[String], FieldFrom)> = paid
            .fields()
            .iter()
            .map(|f| (f.label.as_str(), &f.value[..], f.from))
            .collect();
        let one = |v: &str| vec![v.to_owned()];
        assert_eq!(
            fields,
            [
                ("Pay to", &one("City Power")[..], FieldFrom::You),
                ("Amount", &one("42.10 USD")[..], FieldFrom::Site),
                ("Pay with", &one("Card ending 4242")[..], FieldFrom::You),
                ("Site", &one("site1.example")[..], FieldFrom::Halvo),
            ]
        );
        assert!(card.runs[1].effects.is_empty());
        for state in State::ALL.iter().filter(|s| **s != State::Card) {
            assert!(state_file(*state).card.is_none(), "{}", state.name());
        }
    }
}
