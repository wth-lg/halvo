//! The roster (I1.3, I2): the status of an agent with the case of its line, the settings lines
//! of its configuration, its name, its row in the order of the list, and its card, as pure
//! functions of the facts of I5.1.

use crate::guard::EffectState;
use crate::items::ConfirmState;
use crate::schedule::{finished, holds, needs_person, next_slot, slot_due, Hold};
use crate::types::{
    Answered, BindResult, Card, CardEffect, CardLine, CardRun, Cause, Cleared, ConfigView, Control,
    ControlText, Doing, EffectAnswer, Event, Facts, FullWorkflow, HoldSetCause, ItemId, ItemKind,
    LastCause, Limits, LineKey, LivePhase, OwnItemKind, PastEffect, PastRun, Permission, Raised,
    Row, RunEnd, RunHistory, RunId, Runtime, SignInAnswer, StatusLine, System, Tier, Trigger, What,
};
use crate::types::{Ask, AskAct, AskControl, AskEffect, AskKind, Class, Kept};
use crate::types::{ModelUse, SpendLimit};
use crate::words;

/// The status of the agent of `facts` with the case of its line (I1.3): the first row that fits
/// of an open item that waits for the person, the holds `not_yet`, `off`, `sign_in_refused`
/// (ruling RD-73 4.3), `site_declined` (r4 U2.4a) and `failures`, a live
/// run with its phase, a queued start, a newest run that the agent store has not ended, a last
/// run that ended with a cause of the failure class, said by the sentence of the facts'
/// `last_cause` where they hold one that a row says for the agent's runtime (`FILE_CAUSES` for
/// the runtime file alone, r4 U3.22a), or that its answers are saved where a set-up's answers
/// were stored behind that run's end (r4 U3.22a), a day with `starts_per_day` starts, and the
/// end of the last run. On a day with `starts_per_day` starts, on which step 2 of a start
/// refuses the person's Start now, the row of a failed run says so too (r4 U3.23): saved answers
/// read `problem.answers_saved_today`, and a cause of `START_AGAIN_CAUSES` or a run with no said
/// cause reads `problem.tried_today`. `confirm` is the state of the agent's open confirmation
/// where the coordinator holds it: a confirmation waits for the person only while it is `shown`
/// or `armed` (I1.2, I4.2), and a run that waits for an answered one is `working`. The end of a
/// run is the cause of its newest `run_ended` event, which holds the coordinator's own causes.
pub fn status<C: ConfigView>(
    facts: &Facts<C>,
    limits: &Limits,
    confirm: Option<&ConfirmState>,
) -> StatusLine {
    status_of(facts, limits, confirm, false)
}

/// The status of the agent of `facts` as `status` gives it, where `stuck` says whether the
/// board holds the agent's record stuck (`Board::record_stuck`): such a record's line
/// `problem.record` stands first, so that no row invites a press that is known to fail (I5.7;
/// ruling RD-64 1.5, 2.5).
pub fn status_of<C: ConfigView>(
    facts: &Facts<C>,
    limits: &Limits,
    confirm: Option<&ConfirmState>,
    stuck: bool,
) -> StatusLine {
    if stuck {
        return StatusLine::Record;
    }
    if let Some(open) = facts.doing.item() {
        let waits =
            open.kind != ItemKind::Confirm || confirm.is_none_or(ConfirmState::waits_for_person);
        if waits {
            return StatusLine::NeedsYou(open.kind);
        }
    }
    let standing = holds(facts);
    if crate::schedule::kept_off(facts) && !standing.contains(&Hold::NotYet) {
        return StatusLine::Kept;
    }
    for (hold, line) in [
        (Hold::NotYet, StatusLine::NotYet),
        (Hold::Off, StatusLine::Off),
        (Hold::SignInRefused, StatusLine::SignInRefused),
        (Hold::SiteDeclined, StatusLine::SiteDeclined),
        (Hold::Failures, StatusLine::Failures),
        (Hold::Check, StatusLine::NeedsYou(ItemKind::Check)),
    ] {
        if standing.contains(&hold) {
            return line;
        }
    }
    match facts.doing.phase() {
        Some(LivePhase::Starting) => return StatusLine::Starting,
        Some(LivePhase::Working | LivePhase::Waiting) => return StatusLine::Working,
        Some(LivePhase::Stopping) => return StatusLine::Stopping,
        None => {}
    }
    if matches!(facts.doing, Doing::Queued(_)) {
        return StatusLine::Queued;
    }
    if !facts.store_ended {
        return StatusLine::Store;
    }
    let last = facts.events.iter().rev().find_map(|e| match e.what {
        What::RunEnded { cause, .. } => Some(cause),
        _ => None,
    });
    let used = used_today(facts, limits);
    if let Some(failure) = last.and_then(RunEnd::failure) {
        if answered_since_the_last_end(facts) {
            return if used {
                StatusLine::AnswersSavedToday
            } else {
                StatusLine::AnswersSaved
            };
        }
        let cause = facts
            .last_cause
            .filter(|c| said(*c, facts.config.runtime()));
        return match cause {
            Some(c) if !(used && START_AGAIN_CAUSES.contains(&c)) => StatusLine::LastCause(c),
            _ if used => StatusLine::FailedToday,
            _ => StatusLine::Problem(failure),
        };
    }
    if used {
        return StatusLine::TriedToday;
    }
    match last {
        None => StatusLine::New,
        Some(RunEnd::Completed) => StatusLine::Done,
        Some(RunEnd::NothingToDo) => StatusLine::Nothing,
        Some(_) => StatusLine::StoppedEarly,
    }
}

/// Whether the agent of `facts` has `starts_per_day` `run_started` events with the facts' day, the
/// count of step 2 of a start (I5.5, `schedule::start_step`), which then refuses a start with
/// `budget`, a start of the person among them (r4 U3.23).
fn used_today<C>(facts: &Facts<C>, limits: &Limits) -> bool {
    let today = facts
        .events
        .iter()
        .filter(|e| e.day == facts.today && matches!(e.what, What::RunStarted { .. }))
        .count();
    today >= limits.starts_per_day as usize
}

/// The causes of a failed run whose sentence tells the person to start the agent again, by Start
/// now or by Run the agent again (r4 U3.23, the coordinator's ruling of log line 19955): on a day
/// with `starts_per_day` starts the row of such a cause says `problem.tried_today` in place of
/// that sentence, since step 2 of a start then refuses that start; every other cause keeps its
/// sentence, which names Change its answers or no press.
pub const START_AGAIN_CAUSES: [LastCause; 7] = [
    LastCause::NoChannel,
    LastCause::BrowserStart,
    LastCause::NoBrowser,
    LastCause::Changed,
    LastCause::NotReplaced,
    LastCause::NoAccess,
    LastCause::DataEncoding,
];

/// Whether the agent's rows hold a set-up's `item_answered` behind the end of its newest run, as
/// Use these answers stores it (r4 U3.22a, the fifth ruling): a row of a failed run then reads
/// `problem.answers_saved` in place of the failed run's line and of a last cause's sentence, until
/// the agent's next run ends.
fn answered_since_the_last_end<C>(facts: &Facts<C>) -> bool {
    let ended = facts
        .events
        .iter()
        .rposition(|e| matches!(e.what, What::RunEnded { .. }));
    let answered = facts.events.iter().rposition(|e| {
        matches!(
            &e.what,
            What::ItemAnswered {
                answer: Answered::Setup(_),
                ..
            }
        )
    });
    matches!((ended, answered), (Some(ended), Some(answered)) if answered > ended)
}

/// The causes of a failed run of the runtime file whose sentence a row says for an agent of that
/// runtime alone (r4 U3.22a): the four that the record holds today, the nine finer ones of the
/// agents run's unit r3 U11.3, `no_access` of its unit r3 U11.4, whose sentence is that of the
/// build's system (r4 U3.22d, `StatusLine::key_on`), and `data_encoding` of its unit r3 U11.5; an
/// agent of another runtime with one of these names reads the line of its failed run, as before.
pub const FILE_CAUSES: [LastCause; 15] = [
    LastCause::Workbook,
    LastCause::Sheet,
    LastCause::Changed,
    LastCause::StepFailed,
    LastCause::NoWorkbook,
    LastCause::NoSheet,
    LastCause::NoRange,
    LastCause::DataShape,
    LastCause::NoDataFile,
    LastCause::DataOutside,
    LastCause::DataForm,
    LastCause::NotReplaced,
    LastCause::ReadOnly,
    LastCause::NoAccess,
    LastCause::DataEncoding,
];

/// Whether a row says the sentence of the last cause `cause` for an agent of `runtime`: a cause
/// of `FILE_CAUSES` for the runtime file alone, every other cause for every runtime.
fn said(cause: LastCause, runtime: Runtime) -> bool {
    runtime == Runtime::File || !FILE_CAUSES.contains(&cause)
}

fn text(key: &str, pairs: &[(&str, &str)]) -> Result<String, Cause> {
    words::text(key, pairs)
}

/// The settings lines of a configuration (I2.2), in the order of the table, each whose
/// condition holds and whose words entry exists: `level`, `runs_on`, `asks`, `time` with the
/// minutes of `wall_s` rounded up, `files` and `spend`. The line `runs_on` says that the agent
/// runs on this computer where its stored tier is `on_device` or its runtime asks no model
/// (`asks_a_model`); where the tier is a hosted tier, the runtime asks a model and the app hands
/// the agent's model, that it may use an AI model at the host, with the models, where its stored
/// yes stands, and that the AI model is now at the host and asks again where it does not (r4
/// U3.17b); such an agent with no model handed has no such line.
pub fn lines<C: ConfigView>(config: &C) -> Vec<CardLine> {
    let mut out = Vec::new();
    let mut add = |key: LineKey, words_key: &str, pairs: &[(&str, &str)]| {
        if let Ok(text) = text(words_key, pairs) {
            out.push(CardLine { key, text });
        }
    };
    add(
        LineKey::Level,
        &format!("line.level.{}", config.reasoning().name()),
        &[],
    );
    match (config.tier(), config.model()) {
        (Tier::OnDevice, _) => add(LineKey::RunsOn, "line.runs_on.on_device", &[]),
        _ if !asks_a_model(config) => add(LineKey::RunsOn, "line.runs_on.on_device", &[]),
        (_, Some(model)) if model.stands => add(
            LineKey::RunsOn,
            "line.runs_on.hosted",
            &[("host", &model.host), ("models", &model.models)],
        ),
        (_, Some(model)) => add(
            LineKey::RunsOn,
            "line.runs_on.asks_again",
            &[("host", &model.host)],
        ),
        (_, None) => {}
    }
    add(
        LineKey::Asks,
        &format!("line.asks.{}", config.autonomy().name()),
        &[],
    );
    if config.runtime() != Runtime::None {
        let minutes = config.wall_s().div_ceil(60).to_string();
        add(LineKey::Time, "line.time", &[("minutes", &minutes)]);
    }
    if config.permissions().contains(&Permission::UserFiles) {
        add(LineKey::Files, "line.files", &[]);
    }
    if let Some(budget) = config.budget().filter(|_| config.tier() != Tier::OnDevice) {
        let amount = budget.amount.to_string();
        add(
            LineKey::Spend,
            "line.spend",
            &[("amount", &amount), ("period", budget.period.name())],
        );
    }
    out
}

/// The label of a site value until the app hands its own (I2.3): the host without `www.` and
/// without its last label, its first letter in upper case; none where nothing is left.
pub fn plain_site(site: &str) -> Option<String> {
    let host = site.strip_prefix("www.").unwrap_or(site);
    let (rest, _) = host.rsplit_once('.')?;
    let mut chars = rest.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// The name of an agent of `category` (I2.3): the entry `category.<id>`, joined with the site's
/// label by `name.with_site` where `label` is given.
pub fn name(category: crate::types::Category, label: Option<&str>) -> Result<String, Cause> {
    let base = text(&format!("category.{}", category.name()), &[])?;
    match label {
        Some(site) => text("name.with_site", &[("category", &base), ("site", site)]),
        None => Ok(base),
    }
}

/// The names of the agents of `facts`, in their order, each composed by `name` with the label
/// that `label` gives for its workflow; where two agents have one name, the second and later by
/// agent id get the ordinal ` (2)`, ` (3)` (I2.3).
pub fn names<C>(
    facts: &[&Facts<C>],
    label: impl Fn(&FullWorkflow) -> Option<String>,
) -> Result<Vec<String>, Cause> {
    let plain: Vec<String> = facts
        .iter()
        .map(|f| name(f.workflow.category, label(&f.workflow).as_deref()))
        .collect::<Result<_, _>>()?;
    Ok(plain
        .iter()
        .zip(facts)
        .map(|(n, f)| {
            let before = plain
                .iter()
                .zip(facts)
                .filter(|(m, g)| *m == n && g.agent < f.agent)
                .count();
            if before == 0 {
                n.clone()
            } else {
                format!("{n} ({})", before + 1)
            }
        })
        .collect())
}

/// The controls of a row (I2.5), in the order of `Control`: `run_now` while no hold but
/// `failures`, `sign_in_refused` and `site_declined` stands, no run is live or queued and no
/// set-up item is open, as one that Change its answers put up (r4 U3.22a), and the day has fewer
/// than `starts_per_day` starts (`used_today`), whatever hold stands, since step 2 of a start
/// refuses the person's start on such a day (r4 U3.23); `stop` exactly while a run is live;
/// `take_back` while the agent's start that waits in the queue, whose trigger `waiting` gives,
/// does not come back without the person once taken back: a start of the person, whatever hold
/// stands, unless the agent does not ask first, has a finished run and has a slot due by
/// `limits`, which the next tick would start by itself; never a start of the scheduler (rulings
/// RD-54 (h), RD-149 (a)); `switch_on` while the hold `off`, `sign_in_refused` or `failures`
/// stands (ruling RD-73 4.3), and `switch_off` otherwise; `delete` always.
pub fn controls<C: ConfigView>(
    facts: &Facts<C>,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Vec<Control> {
    let standing = holds(facts);
    let idle = matches!(facts.doing, Doing::Idle(_));
    let live = matches!(facts.doing, Doing::Live(_));
    let set_up = matches!(&facts.doing, Doing::Idle(Some(open)) if open.kind == OwnItemKind::Setup);
    let mut out = Vec::new();
    if idle
        && !set_up
        && !used_today(facts, limits)
        && standing
            .iter()
            .all(|h| matches!(h, Hold::Failures | Hold::SignInRefused | Hold::SiteDeclined))
    {
        out.push(Control::RunNow);
    }
    if live {
        out.push(Control::Stop);
    }
    if waiting == Some(Trigger::Person) && !started_by_the_schedule(facts, limits) {
        out.push(Control::TakeBack);
    }
    if standing
        .iter()
        .any(|h| matches!(h, Hold::Off | Hold::SignInRefused | Hold::Failures))
    {
        out.push(Control::SwitchOn);
    } else {
        out.push(Control::SwitchOff);
    }
    out.push(Control::Delete);
    out
}

/// `offered` with `stop_sign_in` before `delete` where `session` says that the board's member of
/// the agent holds a stored session (I6.1 `session`), the agent is idle (`Doing::Idle`) and
/// `offered` holds `delete`: the control that makes Halvo stop using every sign-in that it keeps
/// for the agent, which the core sends as the body `drop_sessions` (r4 U2.4y). A row with no
/// `delete`, of a gone workflow or of an agent that is being deleted, gets none.
pub fn with_stop_sign_in<C>(
    facts: &Facts<C>,
    session: bool,
    mut offered: Vec<Control>,
) -> Vec<Control> {
    let idle = matches!(facts.doing, Doing::Idle(_));
    if let Some(at) = offered.iter().position(|c| *c == Control::Delete) {
        if session && idle {
            offered.insert(at, Control::StopSignIn);
        }
    }
    offered
}

/// Whether the row of the agent of `facts` offers Change its answers (r4 U3.22a): an agent of the
/// category `spreadsheet_fill`, and of no other in this cut, every question of whose category has
/// a stored answer (the hold `setup` does not stand), that has no run under way, no queued start
/// and no open item (`Doing::Idle` with no item), and whose newest run the agent store has ended.
pub fn offers_change_answers<C: ConfigView>(facts: &Facts<C>) -> bool {
    facts.workflow.category == crate::types::Category::SpreadsheetFill
        && !holds(facts).contains(&Hold::Setup)
        && facts.doing == Doing::Idle(None)
        && facts.store_ended
}

/// `offered` with `change_answers` before the controls of an AI model, `stop_sign_in` and
/// `delete`, where `offered` holds `delete` and `offers_change_answers` holds: a row of a gone
/// workflow or of an agent that is being deleted gets none (r4 U3.22a).
pub fn with_change_answers<C: ConfigView>(
    facts: &Facts<C>,
    mut offered: Vec<Control>,
) -> Vec<Control> {
    let at = offered.iter().position(|c| {
        matches!(
            c,
            Control::UseModel | Control::ThisComputerOnly | Control::StopSignIn | Control::Delete
        )
    });
    if let Some(at) = at.filter(|_| offered.contains(&Control::Delete)) {
        if offers_change_answers(facts) {
            offered.insert(at, Control::ChangeAnswers);
        }
    }
    offered
}

/// THE ONE LIST of the runtimes whose runs ask a model (r4 U3.17b, the coordinator's ruling of
/// log line 18496): only an agent of one of them is offered Use an AI model and is said to be able
/// to use one. A file agent and a calendar agent replay a built-in playbook and ask no model. The
/// unit that lets another runtime ask a model adds it here.
pub const MODEL_RUNTIMES: [Runtime; 2] = [Runtime::Web, Runtime::GraphMail];

/// Whether the agent of `config` has a runtime of `MODEL_RUNTIMES`.
pub fn asks_a_model<C: ConfigView>(config: &C) -> bool {
    MODEL_RUNTIMES.contains(&config.runtime())
}

/// The controls of an agent's use of an AI model (r4 U3.17b), in the order of `Control`: none
/// where the app hands no `ModelUse` (this build's table of hosts holds no row, or its rows name
/// two origins); `use_model` where the agent's runtime asks a model (`MODEL_RUNTIMES`) and its
/// stored tier is `on_device` or its stored yes does not stand; `this_computer_only` where its
/// stored tier is a hosted tier, whatever its runtime: the way back is never withheld.
pub fn model_controls<C: ConfigView>(config: &C) -> Vec<Control> {
    let Some(model) = config.model() else {
        return Vec::new();
    };
    let hosted = config.tier() != Tier::OnDevice;
    let mut out = Vec::new();
    if asks_a_model(config) && (!hosted || !model.stands) {
        out.push(Control::UseModel);
    }
    if hosted {
        out.push(Control::ThisComputerOnly);
    }
    out
}

/// `offered` with the `model_controls` of the agent before `stop_sign_in` and `delete`, where
/// `offered` holds `delete`: a row of a gone workflow or of an agent that is being deleted gets
/// none (r4 U3.17b).
pub fn with_model<C: ConfigView>(facts: &Facts<C>, mut offered: Vec<Control>) -> Vec<Control> {
    if offered.contains(&Control::Delete) {
        if let Some(at) = offered
            .iter()
            .position(|c| matches!(c, Control::StopSignIn | Control::Delete))
        {
            offered.splice(at..at, model_controls(&facts.config));
        }
    }
    offered
}

fn started_by_the_schedule<C: ConfigView>(facts: &Facts<C>, limits: &Limits) -> bool {
    !needs_person(&facts.config) && finished(facts) && slot_due(facts, limits).is_some()
}

/// The row of an agent as the page gets it (I2.5), with its status line, its name, and the
/// trigger of its start that waits in the queue, or none, and the limits of the tick, from
/// which `controls` decides the take-back. The row of `problem.record` offers Stop where a run
/// is live, the delete, and no other control (I1.3; ruling RD-64 1.5, 1.7). It is the row of
/// `row_with_session` for an agent with no stored session.
pub fn row<C: ConfigView>(
    facts: &Facts<C>,
    line: StatusLine,
    name: &str,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Result<Row, Cause> {
    row_with_session(facts, false, line, name, waiting, limits)
}

/// The row of `row`, where `session` says whether the board's member of the agent holds a stored
/// session (I6.1 `session`): its controls are those of `with_stop_sign_in`, `with_model` and
/// `with_change_answers`, so that the row of `problem.record` offers no `stop_sign_in` (r4
/// U2.4y), neither control of an AI model (r4 U3.17b) and no `change_answers` (r4 U3.22a).
pub fn row_with_session<C: ConfigView>(
    facts: &Facts<C>,
    session: bool,
    line: StatusLine,
    name: &str,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Result<Row, Cause> {
    row_on(System::HOST, facts, session, line, name, waiting, limits)
}

/// The row of `row_with_session` with the words of `system` (`StatusLine::key_on`) in place of
/// this build's.
fn row_on<C: ConfigView>(
    system: System,
    facts: &Facts<C>,
    session: bool,
    line: StatusLine,
    name: &str,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Result<Row, Cause> {
    let offered = with_stop_sign_in(facts, session, controls(facts, waiting, limits));
    let offered = with_change_answers(facts, with_model(facts, offered))
        .into_iter()
        .filter(|c| line != StatusLine::Record || matches!(c, Control::Stop | Control::Delete))
        .collect();
    row_offering(system, facts, line, name, offered)
}

fn row_offering<C: ConfigView>(
    system: System,
    facts: &Facts<C>,
    line: StatusLine,
    name: &str,
    offered: Vec<Control>,
) -> Result<Row, Cause> {
    row_with(facts, line.status(), &line.key_on(system), name, offered)
}

fn row_with<C: ConfigView>(
    facts: &Facts<C>,
    status: crate::types::Status,
    key: &str,
    name: &str,
    offered: Vec<Control>,
) -> Result<Row, Cause> {
    let ask = if offered.contains(&Control::Delete) {
        Some(ask(facts, name)?)
    } else {
        None
    };
    let ask_stop_sign_in = if offered.contains(&Control::StopSignIn) {
        Some(stop_sign_in_ask(name)?)
    } else {
        None
    };
    let (ask_use_model, note_use_model) = match facts.config.model() {
        Some(model) if offered.contains(&Control::UseModel) => use_model_ask(&model)?,
        _ => (None, None),
    };
    Ok(Row {
        agent: facts.agent.clone(),
        workflow: facts.workflow.id.clone(),
        name: name.to_owned(),
        status,
        word: text(&format!("status.{}", status.name()), &[])?,
        line: text(key, &[])?,
        say: text(&format!("say.{}", status.name()), &[("name", name)])?,
        level: text(
            &format!("line.level.{}", facts.config.reasoning().name()),
            &[],
        )?,
        next: next_slot(&facts.workflow, facts.today).map(words::day),
        controls: offered
            .into_iter()
            .map(|control| {
                text(&format!("control.{}", control.name()), &[])
                    .map(|text| ControlText { control, text })
            })
            .collect::<Result<_, _>>()?,
        ask,
        ask_stop_sign_in,
        ask_use_model,
        note_use_model,
    })
}

/// The sentences of the question of the delete after its line on the next day, in order
/// (ruling RD-19 (c) rule (4)).
pub const ASK_DELETE: [&str; 7] = [
    "ask.delete.setup",
    "ask.delete.sign_in",
    "ask.delete.records",
    "ask.delete.final",
    "ask.delete.done",
    "ask.delete.copies",
    "ask.delete.starts",
];

fn ask_control(act: AskAct, key: &str) -> Result<AskControl, Cause> {
    Ok(AskControl {
        act,
        text: text(key, &[])?,
    })
}

/// The question that a press on the Delete of the agent's row opens in the page, which sends
/// nothing (ruling RD-19 (c) rules (2) to (4), RD-30 (1)). While a run of the agent is live it
/// is the notice `working`, that the agent must be stopped first and can be removed once it has
/// stopped, with the safe control that leaves it first and the control that sends the row's
/// stop; while the run stops, the notice `stopping` with one control. With no live run it is
/// the question `delete`: its title names the agent, its first sentence the next day where the
/// agent has one, then what the delete removes, that it cannot be brought back, that nothing
/// done or at a website is undone, that the copies beside the person's workbooks stay, and that
/// a task that starts before the answer is stopped first; then each effect of the last run
/// whose outcome is not known (`Facts::unknown`) with its kept fields and the sentence on its
/// record, where the agent's open item `check` names them in its own words instead; the safe
/// control first, then the one that sends the delete. A queued start withholds nothing.
pub fn ask<C: ConfigView>(facts: &Facts<C>, name: &str) -> Result<Ask, Cause> {
    let pair = [("name", name)];
    match facts.doing.phase() {
        Some(LivePhase::Stopping) => Ok(Ask {
            kind: AskKind::Stopping,
            title: text("ask.stopping.title", &pair)?,
            text: vec![text("ask.stopping.text", &[])?],
            effects: Vec::new(),
            record: None,
            controls: vec![ask_control(AskAct::Done, "ask.stopping.done")?],
        }),
        Some(_) => Ok(Ask {
            kind: AskKind::Working,
            title: text("ask.working.title", &pair)?,
            text: vec![text("ask.working.text", &[])?],
            effects: Vec::new(),
            record: None,
            controls: vec![
                ask_control(AskAct::Reject, "ask.working.keep")?,
                ask_control(AskAct::Stop, "control.stop")?,
            ],
        }),
        None => {
            let mut lines = Vec::new();
            if let Some(day) = next_slot(&facts.workflow, facts.today) {
                lines.push(text("ask.delete.next", &[("day", &words::day(day))])?);
            }
            for key in ASK_DELETE {
                lines.push(text(key, &[])?);
            }
            let effects = unknown_effects(facts, name)?;
            let record = if effects.is_empty() {
                None
            } else {
                Some(text("ask.delete.record", &[])?)
            };
            Ok(Ask {
                kind: AskKind::Delete,
                title: text("ask.delete.title", &pair)?,
                text: lines,
                effects,
                record,
                controls: vec![
                    ask_control(AskAct::Reject, "ask.delete.keep")?,
                    ask_control(AskAct::Allow, "control.delete")?,
                ],
            })
        }
    }
}

/// The question that a press on the row's `stop_sign_in` opens in the page, which sends nothing
/// (r4 U2.4y): its title names the agent; it says that Halvo will stop using the sign-in that it
/// keeps for the agent and remove it from this computer (r4 U3.16), and that the agent's next run
/// asks the person to sign in again; then the safe control, and the one that sends
/// `agents_control` with `stop_sign_in`, in the control's own words.
pub fn stop_sign_in_ask(name: &str) -> Result<Ask, Cause> {
    Ok(Ask {
        kind: AskKind::StopSignIn,
        title: text("ask.stop_sign_in.title", &[("name", name)])?,
        text: vec![
            text("ask.stop_sign_in.stops", &[])?,
            text("ask.stop_sign_in.next", &[])?,
        ],
        effects: Vec::new(),
        record: None,
        controls: vec![
            ask_control(AskAct::Reject, "ask.stop_sign_in.keep")?,
            ask_control(AskAct::Allow, "control.stop_sign_in")?,
        ],
    })
}

/// What a press on the row's `use_model` opens in the page, which sends nothing (r4 U3.17b): ONE
/// sentence in place of the question, `ask.use_model.first`, where no key is saved or no limit is
/// set, and else, where the key's state or the count's state could not be read, the sentence of
/// `refused.use_model_not_finished`; else the question `use_model`. Its title asks whether the agent
/// may use an AI model on someone else's computers; its texts say that it would send the text of
/// the page or the message to the host, that it would use the models there, what it never sends,
/// and that the host bills the person's key while Halvo stops at the limit's amount a period, as
/// the limit's view writes them; then the safe control Not now, and Allow for this agent, which
/// sends `agents_control` with `use_model`.
pub fn use_model_ask(model: &ModelUse) -> Result<(Option<Ask>, Option<String>), Cause> {
    let (Some(true), SpendLimit::Stands { amount, period }) = (model.key, &model.limit) else {
        let first = model.key == Some(false) || model.limit == SpendLimit::NotSet;
        let key = if first {
            "ask.use_model.first"
        } else {
            "refused.use_model_not_finished"
        };
        return Ok((None, Some(text(key, &[])?)));
    };
    let host = [("host", model.host.as_str())];
    let ask = Ask {
        kind: AskKind::UseModel,
        title: text("ask.use_model.title", &[])?,
        text: vec![
            text("ask.use_model.sends", &host)?,
            text("ask.use_model.models", &[("models", &model.models)])?,
            text("ask.use_model.never", &[])?,
            text(
                "ask.use_model.bills",
                &[
                    ("host", &model.host),
                    ("amount", amount),
                    ("period", period),
                ],
            )?,
        ],
        effects: Vec::new(),
        record: None,
        controls: vec![
            ask_control(AskAct::Reject, "ask.use_model.keep")?,
            ask_control(AskAct::Allow, "ask.use_model.allow")?,
        ],
    };
    Ok((Some(ask), None))
}

fn unknown_effects<C: ConfigView>(facts: &Facts<C>, name: &str) -> Result<Vec<AskEffect>, Cause> {
    if facts.unknown.is_empty() {
        return Ok(Vec::new());
    }
    let check = facts
        .doing
        .item()
        .is_some_and(|open| open.kind == ItemKind::Check);
    if check {
        let list: Vec<(Class, &[Kept])> = facts
            .unknown
            .iter()
            .map(|u| (u.class, u.kept.as_slice()))
            .collect();
        let title = text("item.check.title", &[("name", name)])?;
        return Ok(vec![AskEffect::checked(title, &list)?]);
    }
    facts
        .unknown
        .iter()
        .map(|u| AskEffect::unknown(u.class, &u.kept))
        .collect()
}

/// The row of an agent whose workflow a tick saw gone while its run lives (I5.7, ruling RD-54
/// (g)): `working.stopping`, or the `working.kill_stuck` line of `system` where `stuck`, the run
/// having outlived a kill that was refused, no control and no next day, since a gone workflow
/// has no slot.
pub fn gone_row<C: ConfigView>(
    facts: &Facts<C>,
    stuck: bool,
    system: System,
    name: &str,
) -> Result<Row, Cause> {
    let line = if stuck {
        StatusLine::KillStuck(system)
    } else {
        StatusLine::Stopping
    };
    Ok(Row {
        next: None,
        ..row_offering(system, facts, line, name, Vec::new())?
    })
}

/// The row of an agent whose delete the person confirmed while its run was live, until that
/// run's end is stored and the delete is sent (I5.7; ruling RD-19 (c) rule 7): `working.deleting`,
/// no control and no next day, since the agent will not start again.
pub fn deleting_row<C: ConfigView>(facts: &Facts<C>, name: &str) -> Result<Row, Cause> {
    Ok(Row {
        next: None,
        ..row_offering(System::HOST, facts, StatusLine::Deleting, name, Vec::new())?
    })
}

/// The newest run of `events` where a Quit named it as not stopped and wrote that list
/// (`run_not_stopped`; ruling RD-150); none where the newest run is another.
pub fn not_stopped_in(events: &[Event]) -> Option<&RunId> {
    let newest = events.iter().rev().find_map(|e| match &e.what {
        What::RunStarted { run, .. } => Some(run),
        _ => None,
    })?;
    events
        .iter()
        .any(|e| matches!(&e.what, What::RunNotStopped { run } if run == newest))
        .then_some(newest)
}

/// The agent's newest run where a Quit named it as not stopped (`not_stopped_in` of its events).
pub fn not_stopped_at_quit<C>(facts: &Facts<C>) -> Option<&RunId> {
    not_stopped_in(&facts.events)
}

/// The end that the store's record of `run` gives, among the records of `facts`, where a row may
/// take it as how that run ended (the fifth named limit of U4.6b; ruling RD-150): any end where
/// this life's `Runs::reap` answered the run `alive` (`answered_alive`), for which no `end_run`
/// is sent; and any end but `failed` otherwise, since the store records the `end_run` of a run
/// that recovery ends as `failed` too. None where no such record names the run.
pub fn recorded_end<C>(facts: &Facts<C>, run: &RunId, answered_alive: bool) -> Option<RunEnd> {
    facts
        .runs
        .iter()
        .find(|r| &r.run == run)
        .map(|r| RunEnd::from(r.outcome))
        .filter(|end| answered_alive || *end != RunEnd::Failed)
}

/// A copy of `facts` in which `run` ends with `end`: each of its `run_ended` rows with
/// `interrupted`, which recovery stored, holds `end`, and where no `run_ended` row of it stands,
/// one of `end` follows its `run_started`; so the row reads the recorded end of that run.
pub fn with_end<C: Clone>(facts: &Facts<C>, run: &RunId, end: RunEnd) -> Facts<C> {
    let mut read = facts.clone();
    let mut ended = false;
    for e in &mut read.events {
        if let What::RunEnded { run: r, cause, .. } = &mut e.what {
            if r == run {
                ended = true;
                if *cause == RunEnd::Interrupted {
                    *cause = end;
                }
            }
        }
    }
    if !ended {
        let at = read
            .events
            .iter()
            .position(|e| matches!(&e.what, What::RunStarted { run: r, .. } if r == run));
        if let Some(k) = at {
            let row = Event {
                what: What::RunEnded {
                    run: run.clone(),
                    cause: end,
                    defers: None,
                },
                ..read.events[k].clone()
            };
            read.events.insert(k + 1, row);
        }
    }
    read
}

/// The row of an agent whose newest run a Quit named as not stopped (`not_stopped_at_quit`;
/// ruling RD-150): the status `problem` with `line.problem.quit`, that its task was still
/// running when Halvo quit and may have gone on, and `offered`, the controls; the row of an
/// agent of a gone workflow passes none and has no next day.
pub fn quit_row<C: ConfigView>(
    facts: &Facts<C>,
    name: &str,
    offered: Vec<Control>,
) -> Result<Row, Cause> {
    row_with(
        facts,
        crate::types::Status::Problem,
        "line.problem.quit",
        name,
        offered,
    )
}

/// The row of an agent of a gone workflow whose newest run a Quit named as not stopped, once the
/// store's record names how that run ended (`recorded_end`): `line`, the line of that end, no
/// control and no next day.
pub fn gone_ended_row<C: ConfigView>(
    facts: &Facts<C>,
    line: StatusLine,
    name: &str,
) -> Result<Row, Cause> {
    Ok(Row {
        next: None,
        ..row_offering(System::HOST, facts, line, name, Vec::new())?
    })
}

/// A row with the time at which its open item was raised, which orders the group `needs_you`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The row.
    pub row: Row,
    /// When the agent's open item was raised, in milliseconds of the clock, or none.
    pub raised: Option<u64>,
}

/// Orders the rows of the list (I2.4): the groups in the order of I1.2; inside `needs_you` the
/// oldest open item first; inside every group then the name in byte order and the agent id.
pub fn order(rows: &mut [Placed]) {
    rows.sort_by(|a, b| {
        let key = |p: &Placed| {
            let item = if p.row.status == crate::types::Status::NeedsYou {
                p.raised.unwrap_or(u64::MAX)
            } else {
                0
            };
            (p.row.status, item)
        };
        key(a)
            .cmp(&key(b))
            .then_with(|| a.row.name.as_bytes().cmp(b.row.name.as_bytes()))
            .then_with(|| a.row.agent.cmp(&b.row.agent))
    });
}

/// Whether the person refused `effect` of `run` (I2.6, ruling RD-85 item 1): the ledger `events`
/// holds an `item_answered` row with `reject_once` for that effect id and that digest in an item
/// whose `item_raised` row names `run`, released or not.
fn said_no(events: &[Event], run: &RunId, effect: &PastEffect) -> bool {
    let items: Vec<&ItemId> = events
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::Confirm(r),
            } if r == run => Some(item),
            _ => None,
        })
        .collect();
    events.iter().any(|e| {
        matches!(&e.what, What::ItemAnswered {
            item,
            answer: Answered::Confirm(entry),
        } if entry.answer == EffectAnswer::RejectOnce
            && entry.effect == effect.effect
            && entry.digest == effect.digest
            && items.contains(&item))
    })
}

/// Whether the person said no at the sign-in question of `run` (I2.6, ruling RD-85 item 3
/// widened by the coordinator's way A): the ledger `events` holds an `item_answered` row with
/// the second step's `decline` in an item whose `item_raised` row of the kind `sign_in` names
/// `run`. The first step's `decline` stops this time and answers no question, and a lapse is no
/// answer: neither is a no here. The hold `site_declined` of the schedule reads it too (r4 U2.4a).
pub(crate) fn said_no_at_sign_in(events: &[Event], run: &RunId) -> bool {
    let items: Vec<&ItemId> = events
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::SignIn(r),
            } if r == run => Some(item),
            _ => None,
        })
        .collect();
    events.iter().any(|e| {
        matches!(&e.what, What::ItemAnswered {
            item,
            answer: Answered::SignIn(SignInAnswer::Second(bind)),
        } if bind.answer == BindResult::Decline && items.contains(&item))
    })
}

/// The end that a run's line says (I2.6, rulings RD-48 (c), RD-65 (e), RD-85 item 3): the cause
/// of the coordinator's own newest `run_ended` row of the run where that cause is one that only
/// the coordinator gives, and the agent store's outcome otherwise; a run that ended `lapsed`
/// after the switch-off that still stands has the line of a stopped run (ruling RD-101 (i)).
fn shown_end(events: &[Event], run: &PastRun) -> RunEnd {
    let ended = events
        .iter()
        .rposition(|e| matches!(&e.what, What::RunEnded { run: r, .. } if *r == run.run));
    let own = ended.and_then(|k| match &events[k].what {
        What::RunEnded { cause, .. } => Some(*cause),
        _ => None,
    });
    let set = events.iter().rposition(|e| {
        matches!(
            e.what,
            What::HoldSet {
                cause: HoldSetCause::Person
            }
        )
    });
    let cleared = events.iter().rposition(|e| {
        matches!(
            e.what,
            What::HoldCleared {
                cause: Cleared::Person
            }
        )
    });
    let switched_off_before = matches!((set, ended), (Some(s), Some(n)) if s < n)
        && set.is_some_and(|s| cleared.is_none_or(|c| c < s));
    match own {
        Some(RunEnd::Lapsed) if switched_off_before => RunEnd::Cancelled,
        Some(
            end @ (RunEnd::Cancelled
            | RunEnd::Lapsed
            | RunEnd::Interrupted
            | RunEnd::Deleted
            | RunEnd::Budget
            | RunEnd::Kind),
        ) => end,
        _ => RunEnd::from(run.outcome),
    }
}

/// One run of a card (I2.6): its day; its line, the sentence of `line.run.<end>` for the end
/// that `shown_end` gives, except that a `declined` run with no effect that the person refused
/// and no no at its sign-in question (`said_no_at_sign_in`) has the neutral `line.run.stopped`
/// (ruling RD-85 item 3); and, in the order of the reply, one
/// entry for each committed effect, by `CardEffect::committed` or, where that refuses it,
/// `CardEffect::unshown`, for each effect whose last row is `rejected` with a null cause, by
/// `CardEffect::not_done`, and for each effect whose last state is `unknown`, whatever its
/// cause, by `CardEffect::unknown` (ruling RD-126 item 10).
fn card_run(events: &[Event], h: &RunHistory) -> Result<CardRun, Cause> {
    let refused = |e: &PastEffect| said_no(events, &h.run.run, e);
    let mut effects = Vec::new();
    for e in &h.effects {
        match (e.state, &e.cause) {
            (EffectState::Committed, _) => effects.push(
                CardEffect::committed(e.class, &e.kept)
                    .or_else(|_| CardEffect::unshown(e.class, &e.kept))?,
            ),
            (EffectState::Rejected, None) => {
                effects.push(CardEffect::not_done(e.class, &e.kept, refused(e))?);
            }
            (EffectState::Unknown, _) => effects.push(CardEffect::unknown(e.class, &e.kept)?),
            _ => {}
        }
    }
    let key = match shown_end(events, &h.run) {
        RunEnd::Declined
            if !h.effects.iter().any(refused) && !said_no_at_sign_in(events, &h.run.run) =>
        {
            "line.run.stopped".to_owned()
        }
        end => format!("line.run.{}", end.name()),
    };
    Ok(CardRun {
        day: words::day(h.run.day),
        text: text(&key, &[])?,
        effects,
    })
}

/// The card of an agent (I2.6): its settings lines and the first `history_runs` runs of
/// `history`, which `Owner::agent_history` gives newest first, each by `card_run` over the
/// agent's ledger rows in `facts`. No single effect fails the card.
pub fn card<C: ConfigView>(
    facts: &Facts<C>,
    name: &str,
    history: &[RunHistory],
    limits: &Limits,
) -> Result<Card, Cause> {
    Ok(Card {
        agent: facts.agent.clone(),
        name: name.to_owned(),
        lines: lines(&facts.config),
        runs: history
            .iter()
            .take(limits.history_runs as usize)
            .map(|h| card_run(&facts.events, h))
            .collect::<Result<_, _>>()?,
        history: None,
    })
}

/// The card's line of the agent's records taken off since Halvo opened, `card.records_dropped`
/// with `{count}` (ruling RD-64 2.6), or none where `count` is 0.
pub fn dropped_line(count: u32) -> Result<Option<CardLine>, Cause> {
    if count == 0 {
        return Ok(None);
    }
    let text = text("card.records_dropped", &[("count", &count.to_string())])?;
    Ok(Some(CardLine {
        key: LineKey::RecordsDropped,
        text,
    }))
}

/// When the agent's open item was raised, the instant of its `item_raised` event, which orders
/// the group `needs_you` (I2.4); none with no open item or no such event.
pub fn raised_at<C>(facts: &Facts<C>) -> Option<u64> {
    let open = facts.doing.item()?;
    facts.events.iter().find_map(|e| match &e.what {
        What::ItemRaised { item, .. } if *item == open.item => u64::try_from(e.at).ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
