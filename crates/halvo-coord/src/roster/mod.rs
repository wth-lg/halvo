//! The roster (I1.3, I2): the status of an agent with the case of its line, the settings lines
//! of its configuration, its name, its row in the order of the list, and its card, as pure
//! functions of the facts of I5.1.

use crate::items::ConfirmState;
use crate::schedule::{holds, next_slot, Hold};
use crate::types::{
    AgentId, Card, CardEffect, CardLine, CardRun, Cause, ConfigView, Control, ControlText, Doing,
    Facts, FullWorkflow, ItemKind, Limits, LineKey, LivePhase, Permission, Row, RunEnd, RunHistory,
    Runtime, StatusLine, Tier, What,
};
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
    if let Some(open) = facts.doing.item() {
        let waits =
            open.kind != ItemKind::Confirm || confirm.is_none_or(ConfirmState::waits_for_person);
        if waits {
            return StatusLine::NeedsYou(open.kind);
        }
    }
    let standing = holds(facts);
    for (hold, line) in [
        (Hold::NotYet, StatusLine::NotYet),
        (Hold::Off, StatusLine::Off),
        (Hold::Failures, StatusLine::Failures),
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
/// `switch_on` while the hold `off` or `failures` stands, and `switch_off` otherwise; `delete`
/// always.
pub fn controls<C: ConfigView>(facts: &Facts<C>) -> Vec<Control> {
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
    if standing.contains(&Hold::Off) || standing.contains(&Hold::Failures) {
        out.push(Control::SwitchOn);
    } else {
        out.push(Control::SwitchOff);
    }
    out.push(Control::Delete);
    out
}

/// The row of an agent as the page gets it (I2.5), with its status line and its name.
pub fn row<C: ConfigView>(facts: &Facts<C>, line: StatusLine, name: &str) -> Result<Row, Cause> {
    let status = line.status();
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
        controls: controls(facts)
            .into_iter()
            .map(|control| {
                text(&format!("control.{}", control.name()), &[])
                    .map(|text| ControlText { control, text })
            })
            .collect::<Result<_, _>>()?,
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

/// The card of an agent (I2.6): its settings lines and the first `history_runs` runs of
/// `history`, which `Owner::agent_history` gives newest first, each with its day, the sentence
/// of `line.run.<end>` and one entry for each of its committed effects in the order of the
/// reply, by `CardEffect::committed` (ruling H-8, filled at this unit's join with U3.2, ruling
/// RD-28 (k)); a committed effect whose kept fields are not those of its class is `malformed`.
pub fn card<C: ConfigView>(
    agent: &AgentId,
    config: &C,
    name: &str,
    history: &[RunHistory],
    limits: &Limits,
) -> Result<Card, Cause> {
    Ok(Card {
        agent: agent.clone(),
        name: name.to_owned(),
        lines: lines(config),
        runs: history
            .iter()
            .take(limits.history_runs as usize)
            .map(|h| {
                let end = RunEnd::from(h.run.outcome);
                let effects = h
                    .effects
                    .iter()
                    .filter(|e| e.state == crate::guard::EffectState::Committed)
                    .map(|e| CardEffect::committed(e.class, &e.kept))
                    .collect::<Result<_, _>>()?;
                text(&format!("line.run.{}", end.name()), &[]).map(|text| CardRun {
                    day: words::day(h.run.day),
                    text,
                    effects,
                })
            })
            .collect::<Result<_, _>>()?,
    })
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
