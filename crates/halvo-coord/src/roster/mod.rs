//! The roster (I1.3, I2): the status of an agent with the case of its line, the settings lines
//! of its configuration, its name, its row in the order of the list, and its card, as pure
//! functions of the facts of I5.1.

use crate::guard::EffectState;
use crate::items::ConfirmState;
use crate::schedule::{finished, holds, needs_person, next_slot, slot_due, Hold};
use crate::types::{
    Answered, Card, CardEffect, CardLine, CardRun, Cause, Cleared, ConfigView, Control,
    ControlText, Doing, EffectAnswer, Event, Facts, FullWorkflow, HoldSetCause, ItemId, ItemKind,
    Limits, LineKey, LivePhase, PastEffect, PastRun, Permission, Raised, Row, RunEnd, RunHistory,
    RunId, Runtime, StatusLine, System, Tier, Trigger, What,
};
use crate::types::{Ask, AskAct, AskControl, AskEffect, AskKind, Class, Kept};
use crate::words;

/// The status of the agent of `facts` with the case of its line (I1.3): the first row that fits
/// of an open item that waits for the person, the holds `not_yet`, `off` and `failures`, a live
/// run with its phase, a queued start, a newest run that the agent store has not ended, a last
/// run that ended with a cause of the failure class, a day with `starts_per_day` starts, and the
/// end of the last run. `confirm` is the state of the agent's open confirmation where the
/// coordinator holds it: a confirmation waits for the person only while it is `shown` or `armed`
/// (I1.2, I4.2), and a run that waits for an answered one is `working`. The end of a run is the
/// cause of its newest `run_ended` event, which holds the coordinator's own causes.
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
    if let Some(failure) = last.and_then(RunEnd::failure) {
        return StatusLine::Problem(failure);
    }
    let today = facts
        .events
        .iter()
        .filter(|e| e.day == facts.today && matches!(e.what, What::RunStarted { .. }))
        .count();
    if today >= limits.starts_per_day as usize {
        return StatusLine::TriedToday;
    }
    match last {
        None => StatusLine::New,
        Some(RunEnd::Completed) => StatusLine::Done,
        Some(RunEnd::NothingToDo) => StatusLine::Nothing,
        Some(_) => StatusLine::StoppedEarly,
    }
}

fn text(key: &str, pairs: &[(&str, &str)]) -> Result<String, Cause> {
    words::text(key, pairs)
}

/// The settings lines of a configuration (I2.2), in the order of the table, each whose
/// condition holds and whose words entry exists: `level`, `runs_on`, `asks`, `time` with the
/// minutes of `wall_s` rounded up, `files` and `spend`.
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
    add(
        LineKey::RunsOn,
        &format!("line.runs_on.{}", config.tier().name()),
        &[],
    );
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
/// `failures` stands and no run is live or queued; `stop` exactly while a run is live;
/// `take_back` while the agent's start that waits in the queue, whose trigger `waiting` gives,
/// does not come back without the person once taken back: a start of the person, whatever hold
/// stands, unless the agent does not ask first, has a finished run and has a slot due by
/// `limits`, which the next tick would start by itself; never a start of the scheduler (rulings
/// RD-54 (h), RD-149 (a)); `switch_on` while the hold `off` or `failures` stands, and
/// `switch_off` otherwise; `delete` always.
pub fn controls<C: ConfigView>(
    facts: &Facts<C>,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Vec<Control> {
    let standing = holds(facts);
    let idle = matches!(facts.doing, Doing::Idle(_));
    let live = matches!(facts.doing, Doing::Live(_));
    let mut out = Vec::new();
    if idle && standing.iter().all(|h| *h == Hold::Failures) {
        out.push(Control::RunNow);
    }
    if live {
        out.push(Control::Stop);
    }
    if waiting == Some(Trigger::Person) && !started_by_the_schedule(facts, limits) {
        out.push(Control::TakeBack);
    }
    if standing.contains(&Hold::Off) || standing.contains(&Hold::Failures) {
        out.push(Control::SwitchOn);
    } else {
        out.push(Control::SwitchOff);
    }
    out.push(Control::Delete);
    out
}

fn started_by_the_schedule<C: ConfigView>(facts: &Facts<C>, limits: &Limits) -> bool {
    !needs_person(&facts.config) && finished(facts) && slot_due(facts, limits).is_some()
}

/// The row of an agent as the page gets it (I2.5), with its status line, its name, and the
/// trigger of its start that waits in the queue, or none, and the limits of the tick, from
/// which `controls` decides the take-back. The row of `problem.record` offers Stop where a run
/// is live, the delete, and no other control (I1.3; ruling RD-64 1.5, 1.7).
pub fn row<C: ConfigView>(
    facts: &Facts<C>,
    line: StatusLine,
    name: &str,
    waiting: Option<Trigger>,
    limits: &Limits,
) -> Result<Row, Cause> {
    let offered = controls(facts, waiting, limits)
        .into_iter()
        .filter(|c| line != StatusLine::Record || matches!(c, Control::Stop | Control::Delete))
        .collect();
    row_offering(facts, line, name, offered)
}

fn row_offering<C: ConfigView>(
    facts: &Facts<C>,
    line: StatusLine,
    name: &str,
    offered: Vec<Control>,
) -> Result<Row, Cause> {
    let status = line.status();
    let ask = if offered.contains(&Control::Delete) {
        Some(ask(facts, name)?)
    } else {
        None
    };
    Ok(Row {
        agent: facts.agent.clone(),
        workflow: facts.workflow.id.clone(),
        name: name.to_owned(),
        status,
        word: text(&format!("status.{}", status.name()), &[])?,
        line: text(&line.key(), &[])?,
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
        ..row_offering(facts, line, name, Vec::new())?
    })
}

/// The row of an agent whose delete the person confirmed while its run was live, until that
/// run's end is stored and the delete is sent (I5.7; ruling RD-19 (c) rule 7): `working.deleting`,
/// no control and no next day, since the agent will not start again.
pub fn deleting_row<C: ConfigView>(facts: &Facts<C>, name: &str) -> Result<Row, Cause> {
    Ok(Row {
        next: None,
        ..row_offering(facts, StatusLine::Deleting, name, Vec::new())?
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
/// has the neutral `line.run.stopped` (ruling RD-85 item 3); and, in the order of the reply, one
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
        RunEnd::Declined if !h.effects.iter().any(refused) => "line.run.stopped".to_owned(),
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
