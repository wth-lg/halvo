//! The simulation of contract T7.5: six cases on fake accounts that move only through
//! `halvo_route::spend::step`, with `admit::fits` on a `Room` made by `Room::of` as the admission
//! where a case has one. No model runs and nothing is sent.
//!
//! `simulate(seed)` runs the cases in the order of `CASES` from `SplitMix64::new(seed)`, with the
//! compiled-in prices and default cap. Its draws, in the order that the cases make them: a request
//! draws its tier (`below(2)`, the two hosted tiers in the order of `Tier::ALL`), the length of its
//! one `user` message (`1 + below(8192)` bytes of `x`) and its output limit (`1 + below(256)`); a
//! task draws its three requests when it starts; a settle draws its input tokens
//! (`below(body_bytes + 1)`) and then its output tokens (`below(limit + 1)`). `fan_out` draws, for
//! each repetition, the cap (`10 + below(91)` minor units), the eight tasks, the order of their
//! admission (a Fisher-Yates shuffle from the last place down, `below(place + 1)`) and, before each
//! move, the admitted task that makes it (`below(tasks with moves left)`). `cap_lowered` draws a
//! fourth and later request of its first task when it needs one; `error_burst` draws one request
//! for each task. The other device of `second_device` draws nothing: each of its holds is the
//! largest request that fits the room left, the tier `frontier` before `low_cost_hosted`, an
//! output limit of 1 and the longest message from 1 to 8,192 bytes, settled at its whole amount,
//! until no request of the smallest body fits; so the room left is then below every hold that a
//! task can ask.

use std::io::Write;
use std::num::NonZeroU64;

use halvo_route::admit::{fits, Fit, Room};
use halvo_route::prices::{Micro, Prices, MICRO_PER_MINOR};
use halvo_route::release;
use halvo_route::spend::{step, Account, Hold, Move, SpendCause};
use halvo_route::tier::Tier;
use halvo_route_wire::{Body, Effort, Message, Role, WireTier};

use crate::generate::SplitMix64;
use crate::out::{self, Refused};

#[cfg(test)]
mod tests;

/// The names of the six cases, in the order in which they run and print.
pub const CASES: [&str; 6] = [
    "near_cap",
    "fan_out",
    "cap_lowered",
    "stream_cut",
    "error_burst",
    "second_device",
];

/// The cap of `near_cap` in minor units.
pub const NEAR_CAP_MINOR: u64 = 100;
/// The cap of `second_device` in minor units.
pub const SECOND_DEVICE_MINOR: u64 = 200;
/// The repetitions of `fan_out` and the tasks of each.
pub const FAN_OUT_REPETITIONS: u64 = 30;
pub const FAN_OUT_TASKS: usize = 8;
/// The requests of a task.
pub const TASK_REQUESTS: usize = 3;
/// The tasks of each half of `error_burst`.
pub const BURST_TASKS: u64 = 20;
/// The sends of a request of the second half of `error_burst`, each with a new id.
pub const BURST_SENDS: u64 = 3;
/// What `cap_lowered` spends, in micro-units, before its cap is lowered.
pub const LOWER_AFTER: Micro = 10_000;
/// The moves after which a case that has not ended stops with a fault.
pub const MOST_MOVES: u64 = 100_000;
/// The longest message of a request, in bytes, and the highest output limit.
pub const MOST_CONTENT: u64 = 8_192;
pub const MOST_OUTPUT: u64 = 256;

const PERIOD_END_MS: u64 = 0;

/// The simulation's own match from a tier of the router to a tier of the wire.
pub fn wire_tier(tier: Tier) -> Option<WireTier> {
    match tier {
        Tier::OnDevice => None,
        Tier::LowCostHosted => Some(WireTier::LowCostHosted),
        Tier::Frontier => Some(WireTier::Frontier),
    }
}

/// One request: its tier, the bytes of its body as `Body::to_bytes` writes it, its output limit
/// and its hold by `Prices::hold`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request {
    pub tier: Tier,
    pub body_bytes: u64,
    pub max_output_tokens: u64,
    pub amount: Micro,
}

/// One move of a case: the line of the sequences file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub case: &'static str,
    pub account: u64,
    pub step: u64,
    pub hold: u64,
    pub mv: Move,
    pub request: Option<Request>,
    pub answer: Result<(), SpendCause>,
    pub cap_minor: Option<u64>,
    pub spent: Micro,
    pub held: Micro,
}

impl Step {
    /// The line of the sequences file, without its LF.
    pub fn line(&self) -> String {
        let arg = match self.mv {
            Move::Reserve { amount } => amount.to_string(),
            Move::Settle { cost } => cost.to_string(),
            Move::Dispatch | Move::Void | Move::Expire => "null".to_owned(),
        };
        let (tier, body_bytes, max_output_tokens) = match self.request {
            Some(request) => (
                format!("\"{}\"", request.tier.as_str()),
                request.body_bytes.to_string(),
                request.max_output_tokens.to_string(),
            ),
            None => ("null".to_owned(), "null".to_owned(), "null".to_owned()),
        };
        let answer = match self.answer {
            Ok(()) => "ok",
            Err(cause) => cause.as_str(),
        };
        let cap = self
            .cap_minor
            .map_or_else(|| "null".to_owned(), |cap| cap.to_string());
        format!(
            "{{\"case\":\"{}\",\"account\":{},\"step\":{},\"hold\":{},\"move\":\"{}\",\"arg\":{arg},\
             \"tier\":{tier},\"body_bytes\":{body_bytes},\"max_output_tokens\":{max_output_tokens},\
             \"answer\":\"{answer}\",\"cap\":{cap},\"spent\":{},\"held\":{}}}",
            self.case,
            self.account,
            self.step,
            self.hold,
            self.mv.name(),
            self.spent,
            self.held
        )
    }
}

/// The counts of one case, as its line prints them.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Counts {
    pub tasks: u64,
    pub reserves: u64,
    pub admitted: u64,
    pub refused_over_cap: u64,
    pub expired: u64,
    pub overruns: u64,
    pub above_cap: u64,
    pub stopped_mid_run: u64,
}

/// What a run of the six cases found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub cases: Vec<(&'static str, Counts)>,
    pub admit_sum_ok: bool,
    pub faults: u64,
    pub steps: Vec<Step>,
}

impl Outcome {
    /// `pass` when no settled hold overran, no `Reserve` went through above the cap in force,
    /// every admission held the sum, and no fault stopped a case: a request whose body could not
    /// be built, or a case that had not ended after `MOST_MOVES` moves.
    pub fn passed(&self) -> bool {
        let total = self.total();
        total.overruns == 0 && total.above_cap == 0 && self.admit_sum_ok && self.faults == 0
    }

    fn total(&self) -> Counts {
        let mut total = Counts::default();
        for (_, counts) in &self.cases {
            total.overruns += counts.overruns;
            total.above_cap += counts.above_cap;
        }
        total
    }

    /// The seven lines of T7.5.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .cases
            .iter()
            .map(|(name, c)| {
                format!(
                    "spend case={name} tasks={} reserves={} admitted={} refused_over_cap={} \
                     expired={} overruns={} above_cap={} stopped_mid_run={}",
                    c.tasks,
                    c.reserves,
                    c.admitted,
                    c.refused_over_cap,
                    c.expired,
                    c.overruns,
                    c.above_cap,
                    c.stopped_mid_run
                )
            })
            .collect();
        let total = self.total();
        lines.push(format!(
            "spend cases={} overruns={} above_cap={} admit_sum={} result={}",
            self.cases.len(),
            total.overruns,
            total.above_cap,
            if self.admit_sum_ok { "ok" } else { "miss" },
            if self.passed() { "pass" } else { "fail" }
        ));
        lines
    }

    /// The sequences file: one line per move, each with its LF.
    pub fn export(&self) -> String {
        let mut text = String::new();
        for step in &self.steps {
            text.push_str(&step.line());
            text.push('\n');
        }
        text
    }
}

/// The six cases from `seed`, with the compiled-in prices and default cap.
pub fn simulate(seed: u64) -> Outcome {
    simulate_with(
        seed,
        Prices::release(),
        release::cap().map(|cap| cap.amount),
    )
}

/// The six cases from `seed` with the given prices and default cap in minor units.
pub fn simulate_with(seed: u64, prices: &Prices, default_cap: Option<u64>) -> Outcome {
    run(Sim::new(seed, prices, default_cap, step))
}

type Machine = fn(&Account, &Hold, Move) -> Result<(Account, Hold), SpendCause>;
type FitRule = fn(&Room, Micro, &[Micro]) -> Fit;
type DrawRule = fn(&mut SplitMix64, NonZeroU64) -> u64;

fn run(mut sim: Sim<'_>) -> Outcome {
    for case in CASES {
        sim.start(case);
        match case {
            "near_cap" => sim.near_cap(),
            "fan_out" => sim.fan_out(),
            "cap_lowered" => sim.cap_lowered(),
            "stream_cut" => sim.stream_cut(),
            "error_burst" => sim.error_burst(),
            _ => sim.second_device(),
        }
        sim.outcome.cases.push((case, sim.counts));
    }
    sim.outcome
}

/// `route-gate spend-sim --seed N [--export FILE]`: prints the seven lines and, with `--export`,
/// writes the sequences file as a new file. Exit status 0 for `result=pass`, 1 for `result=fail`,
/// 2 for a refusal (`usage`, `file`) with one line on `err` and nothing on `out`.
pub fn command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    command_on(args, out, err, simulate)
}

fn command_on(
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
    simulate: fn(u64) -> Outcome,
) -> u8 {
    let (seed, export) = match options(args) {
        Ok(found) => found,
        Err(detail) => {
            let _ = writeln!(err, "REFUSED usage: {detail}");
            return 2;
        }
    };
    let outcome = simulate(seed);
    if let Some(path) = export {
        let refused = match out::write_new(path, &outcome.export(), || {}) {
            Ok(()) => None,
            Err(Refused::Exists) => Some("--export exists"),
            Err(Refused::Unwritable) => Some("--export cannot be written"),
        };
        if let Some(detail) = refused {
            let _ = writeln!(err, "REFUSED file: {detail}");
            return 2;
        }
    }
    let mut text = outcome.lines().join("\n");
    text.push('\n');
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) if outcome.passed() => 0,
        _ => 1,
    }
}

fn options(args: &[String]) -> Result<(u64, Option<&str>), String> {
    let mut seed = None;
    let mut export = None;
    let mut words = args.iter();
    while let Some(name) = words.next() {
        let slot = match name.as_str() {
            "--seed" => &mut seed,
            "--export" => &mut export,
            _ => return Err(format!("unknown argument {}", printable(name))),
        };
        let Some(value) = words.next() else {
            return Err(format!("{name} has no value"));
        };
        if slot.replace(value.as_str()).is_some() {
            return Err(format!("{name} twice"));
        }
    }
    let Some(text) = seed else {
        return Err("--seed is required".to_owned());
    };
    let canonical =
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u64>() {
        Ok(seed) if canonical => Ok((seed, export)),
        _ => Err("--seed takes a whole number from 1".to_owned()),
    }
}

fn printable(word: &str) -> String {
    if word.starts_with("--") && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        word.to_owned()
    } else {
        "of another form".to_owned()
    }
}

/// `spent + held` above the cap of the account, in u128; an account with no cap holds nothing
/// under a cap, so every amount stands above it.
pub fn above_cap(account: &Account) -> bool {
    let counted = u128::from(account.spent) + u128::from(account.held);
    match account.cap_minor {
        Some(cap) => counted > u128::from(cap) * u128::from(MICRO_PER_MINOR),
        None => counted > 0,
    }
}

/// The admission's sum: the estimate and the running estimates sum to at most the room that the
/// room's four values leave, computed here in u128 and not by `Room::left`.
pub fn sum_within(room: &Room, estimate: Micro, running: &[Micro]) -> bool {
    let sum = running.iter().map(|each| u128::from(*each)).sum::<u128>() + u128::from(estimate);
    let cap = u128::from(room.cap_minor) * u128::from(MICRO_PER_MINOR);
    let used = u128::from(room.spent) + u128::from(room.held);
    sum <= cap.saturating_sub(used)
}

struct Fake {
    number: u64,
    account: Account,
}

struct Slot {
    number: u64,
    hold: Hold,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ended {
    Done,
    Refused(SpendCause),
}

struct Task {
    requests: Vec<Request>,
    admitted: bool,
    next: usize,
    phase: usize,
    slot: Option<Slot>,
    ended: bool,
}

impl Task {
    fn new(requests: Vec<Request>, admitted: bool) -> Task {
        Task {
            requests,
            admitted,
            next: 0,
            phase: 0,
            slot: None,
            ended: false,
        }
    }

    fn estimate(&self) -> Micro {
        self.requests
            .iter()
            .fold(0, |sum: Micro, request| sum.saturating_add(request.amount))
    }
}

struct Sim<'a> {
    prices: &'a Prices,
    machine: Machine,
    fit: FitRule,
    draw: DrawRule,
    draws: SplitMix64,
    default_cap: Option<u64>,
    outcome: Outcome,
    case: &'static str,
    counts: Counts,
    step: u64,
    holds: u64,
    accounts: u64,
}

impl<'a> Sim<'a> {
    fn new(seed: u64, prices: &'a Prices, default_cap: Option<u64>, machine: Machine) -> Sim<'a> {
        Sim {
            prices,
            machine,
            fit: fits,
            draw: SplitMix64::below,
            draws: SplitMix64::new(seed),
            default_cap,
            outcome: Outcome {
                cases: Vec::new(),
                admit_sum_ok: true,
                faults: 0,
                steps: Vec::new(),
            },
            case: CASES[0],
            counts: Counts::default(),
            step: 0,
            holds: 0,
            accounts: 0,
        }
    }

    fn start(&mut self, case: &'static str) {
        self.case = case;
        self.counts = Counts::default();
        self.step = 0;
        self.holds = 0;
        self.accounts = 0;
    }

    fn unended(&mut self) -> bool {
        if self.step < MOST_MOVES {
            return false;
        }
        self.outcome.faults += 1;
        true
    }

    fn below(&mut self, bound: u64) -> u64 {
        match NonZeroU64::new(bound) {
            Some(bound) => (self.draw)(&mut self.draws, bound),
            None => 0,
        }
    }

    fn account(&mut self, cap_minor: Option<u64>) -> Fake {
        self.accounts += 1;
        Fake {
            number: self.accounts,
            account: Account {
                cap_minor,
                active: true,
                spent: 0,
                held: 0,
            },
        }
    }

    fn slot(&mut self) -> Slot {
        self.holds += 1;
        Slot {
            number: self.holds,
            hold: Hold::new(),
        }
    }

    fn body_bytes(&self, tier: Tier, length: u64, limit: u64) -> Option<u64> {
        let body = Body {
            tier: wire_tier(tier)?,
            effort: Effort::Medium,
            max_output_tokens: u32::try_from(limit).ok()?,
            messages: vec![Message {
                role: Role::User,
                content: "x".repeat(usize::try_from(length).ok()?),
            }],
        };
        let bytes = body.to_bytes().ok()?;
        u64::try_from(bytes.len()).ok()
    }

    fn built(&mut self, tier: Tier, length: u64, limit: u64) -> Option<Request> {
        let Some(body_bytes) = self.body_bytes(tier, length, limit) else {
            self.outcome.faults += 1;
            return None;
        };
        Some(Request {
            tier,
            body_bytes,
            max_output_tokens: limit,
            amount: self.prices.hold(tier, body_bytes, limit),
        })
    }

    fn request(&mut self) -> Option<Request> {
        let tier = if self.below(2) == 0 {
            Tier::LowCostHosted
        } else {
            Tier::Frontier
        };
        let length = 1 + self.below(MOST_CONTENT);
        let limit = 1 + self.below(MOST_OUTPUT);
        self.built(tier, length, limit)
    }

    fn task_requests(&mut self) -> Option<Vec<Request>> {
        let mut requests = Vec::with_capacity(TASK_REQUESTS);
        for _ in 0..TASK_REQUESTS {
            requests.push(self.request()?);
        }
        Some(requests)
    }

    fn cost(&mut self, request: &Request) -> Micro {
        let tokens_in = self.below(request.body_bytes + 1);
        let tokens_out = self.below(request.max_output_tokens + 1);
        self.prices.cost(request.tier, tokens_in, tokens_out)
    }

    fn act(
        &mut self,
        fake: &mut Fake,
        slot: &mut Slot,
        mv: Move,
        request: Option<Request>,
    ) -> Result<(), SpendCause> {
        self.step += 1;
        let answer = match (self.machine)(&fake.account, &slot.hold, mv) {
            Ok((account, hold)) => {
                fake.account = account;
                slot.hold = hold;
                Ok(())
            }
            Err(cause) => Err(cause),
        };
        match (mv, answer) {
            (Move::Reserve { .. }, Ok(())) => {
                self.counts.reserves += 1;
                if above_cap(&fake.account) {
                    self.counts.above_cap += 1;
                }
            }
            (Move::Reserve { .. }, Err(cause)) => {
                self.counts.reserves += 1;
                if cause == SpendCause::OverCap {
                    self.counts.refused_over_cap += 1;
                }
            }
            (Move::Expire, Ok(())) => self.counts.expired += 1,
            (Move::Settle { .. }, Ok(())) if slot.hold.overrun() > 0 => {
                self.counts.overruns += 1;
            }
            _ => {}
        }
        self.outcome.steps.push(Step {
            case: self.case,
            account: fake.number,
            step: self.step,
            hold: slot.number,
            mv,
            request: match mv {
                Move::Reserve { .. } => request,
                _ => None,
            },
            answer,
            cap_minor: fake.account.cap_minor,
            spent: fake.account.spent,
            held: fake.account.held,
        });
        answer
    }

    fn reserve(
        &mut self,
        fake: &mut Fake,
        slot: &mut Slot,
        request: Request,
    ) -> Result<(), SpendCause> {
        self.act(
            fake,
            slot,
            Move::Reserve {
                amount: request.amount,
            },
            Some(request),
        )
    }

    fn settle(&mut self, fake: &mut Fake, slot: &mut Slot, request: &Request) {
        let cost = self.cost(request);
        let _ = self.act(fake, slot, Move::Settle { cost }, None);
    }

    fn whole(&mut self, fake: &mut Fake, request: Request) -> Ended {
        let mut slot = self.slot();
        if let Err(cause) = self.reserve(fake, &mut slot, request) {
            return Ended::Refused(cause);
        }
        let _ = self.act(fake, &mut slot, Move::Dispatch, None);
        self.settle(fake, &mut slot, &request);
        Ended::Done
    }

    fn refused(&mut self, task: &Task, index: usize, cause: SpendCause) {
        if task.admitted && index > 0 && cause == SpendCause::OverCap {
            self.counts.stopped_mid_run += 1;
        }
    }

    fn run_task(&mut self, fake: &mut Fake, task: &mut Task) -> Ended {
        while task.next < task.requests.len() {
            let index = task.next;
            task.next += 1;
            if let Ended::Refused(cause) = self.whole(fake, task.requests[index]) {
                self.refused(task, index, cause);
                return Ended::Refused(cause);
            }
        }
        Ended::Done
    }

    fn move_of(&mut self, fake: &mut Fake, task: &mut Task) {
        let index = task.next;
        let request = task.requests[index];
        let mut slot = match task.slot.take() {
            Some(slot) => slot,
            None => self.slot(),
        };
        match task.phase {
            0 => {
                if let Err(cause) = self.reserve(fake, &mut slot, request) {
                    self.refused(task, index, cause);
                    task.ended = true;
                    return;
                }
                task.phase = 1;
                task.slot = Some(slot);
            }
            1 => {
                let _ = self.act(fake, &mut slot, Move::Dispatch, None);
                task.phase = 2;
                task.slot = Some(slot);
            }
            _ => {
                self.settle(fake, &mut slot, &request);
                task.phase = 0;
                task.next += 1;
                task.ended = task.next == task.requests.len();
            }
        }
    }

    fn admit(&mut self, room: &Room, task: &Task, running: &mut Vec<Micro>) -> bool {
        self.admit_by(self.fit, room, task, running)
    }

    fn admit_by(
        &mut self,
        fit: FitRule,
        room: &Room,
        task: &Task,
        running: &mut Vec<Micro>,
    ) -> bool {
        let estimate = task.estimate();
        if fit(room, estimate, running) != Fit::Fits {
            return false;
        }
        if !sum_within(room, estimate, running) {
            self.outcome.admit_sum_ok = false;
        }
        running.push(estimate);
        self.counts.admitted += 1;
        true
    }

    fn near_cap(&mut self) {
        let mut fake = self.account(Some(NEAR_CAP_MINOR));
        loop {
            if self.unended() {
                return;
            }
            let Some(requests) = self.task_requests() else {
                return;
            };
            self.counts.tasks += 1;
            let mut task = Task::new(requests, false);
            if self.run_task(&mut fake, &mut task) != Ended::Done {
                return;
            }
        }
    }

    fn fan_out(&mut self) {
        for _ in 0..FAN_OUT_REPETITIONS {
            let cap = 10 + self.below(91);
            let mut fake = self.account(Some(cap));
            let mut tasks = Vec::with_capacity(FAN_OUT_TASKS);
            for _ in 0..FAN_OUT_TASKS {
                let Some(requests) = self.task_requests() else {
                    return;
                };
                tasks.push(Task::new(requests, false));
            }
            self.counts.tasks += FAN_OUT_TASKS as u64;
            let mut order: Vec<usize> = (0..FAN_OUT_TASKS).collect();
            for place in (1..FAN_OUT_TASKS).rev() {
                let other = self.below(place as u64 + 1) as usize;
                order.swap(place, other);
            }
            let Some(room) = Room::of(&fake.account, PERIOD_END_MS) else {
                continue;
            };
            let mut running = Vec::new();
            let mut admitted = Vec::new();
            for index in order {
                if self.admit(&room, &tasks[index], &mut running) {
                    tasks[index].admitted = true;
                    admitted.push(index);
                }
            }
            loop {
                let open: Vec<usize> = admitted
                    .iter()
                    .copied()
                    .filter(|index| !tasks[*index].ended)
                    .collect();
                if open.is_empty() {
                    break;
                }
                let pick = open[self.below(open.len() as u64) as usize];
                self.move_of(&mut fake, &mut tasks[pick]);
            }
        }
    }

    fn cap_lowered(&mut self) {
        let mut fake = self.account(self.default_cap);
        let Some(first) = self.task_requests() else {
            return;
        };
        self.counts.tasks += 1;
        let mut queue = first.into_iter();
        while fake.account.spent < LOWER_AFTER {
            if self.unended() {
                return;
            }
            let request = match queue.next() {
                Some(request) => request,
                None => match self.request() {
                    Some(request) => request,
                    None => return,
                },
            };
            if self.whole(&mut fake, request) != Ended::Done {
                return;
            }
        }
        let Some(second) = self.task_requests() else {
            return;
        };
        self.counts.tasks += 1;
        let mut slot = self.slot();
        if self.reserve(&mut fake, &mut slot, second[0]).is_err() {
            return;
        }
        let _ = self.act(&mut fake, &mut slot, Move::Dispatch, None);
        fake.account.cap_minor = Some(fake.account.spent / MICRO_PER_MINOR);
        self.settle(&mut fake, &mut slot, &second[0]);
        let mut task = Task::new(second, false);
        task.next = 1;
        let _ = self.run_task(&mut fake, &mut task);
    }

    fn stream_cut(&mut self) {
        let mut fake = self.account(self.default_cap);
        let Some(request) = self.request() else {
            return;
        };
        self.counts.tasks += 1;
        let mut slot = self.slot();
        if self.reserve(&mut fake, &mut slot, request).is_err() {
            return;
        }
        let _ = self.act(&mut fake, &mut slot, Move::Dispatch, None);
        let _ = self.act(&mut fake, &mut slot, Move::Expire, None);
    }

    fn error_burst(&mut self) {
        let mut same_id = self.account(self.default_cap);
        for _ in 0..BURST_TASKS {
            let Some(request) = self.request() else {
                return;
            };
            self.counts.tasks += 1;
            let mut slot = self.slot();
            if self.reserve(&mut same_id, &mut slot, request).is_err() {
                continue;
            }
            let _ = self.act(&mut same_id, &mut slot, Move::Dispatch, None);
            let _ = self.act(&mut same_id, &mut slot, Move::Expire, None);
            let _ = self.reserve(&mut same_id, &mut slot, request);
            let _ = self.act(&mut same_id, &mut slot, Move::Dispatch, None);
        }
        let mut new_ids = self.account(self.default_cap);
        for _ in 0..BURST_TASKS {
            let Some(request) = self.request() else {
                return;
            };
            self.counts.tasks += 1;
            for _ in 0..BURST_SENDS {
                let mut slot = self.slot();
                if self.reserve(&mut new_ids, &mut slot, request).is_err() {
                    break;
                }
                let _ = self.act(&mut new_ids, &mut slot, Move::Dispatch, None);
                let _ = self.act(&mut new_ids, &mut slot, Move::Expire, None);
            }
        }
    }

    fn second_device(&mut self) {
        let mut fake = self.account(Some(SECOND_DEVICE_MINOR));
        let mut tasks = Vec::with_capacity(2);
        for _ in 0..2 {
            let Some(requests) = self.task_requests() else {
                return;
            };
            tasks.push(Task::new(requests, false));
        }
        self.counts.tasks += 2;
        let Some(room) = Room::of(&fake.account, PERIOD_END_MS) else {
            return;
        };
        let mut running = Vec::new();
        for task in &mut tasks {
            task.admitted = self.admit(&room, task, &mut running);
        }
        for task in tasks.iter_mut().filter(|task| task.admitted) {
            if self.whole(&mut fake, task.requests[0]) != Ended::Done {
                task.ended = true;
            }
            task.next = 1;
        }
        while let Some(request) = self.largest_fit(&fake.account) {
            if self.unended() {
                return;
            }
            let mut slot = self.slot();
            if self.reserve(&mut fake, &mut slot, request).is_err() {
                break;
            }
            let _ = self.act(&mut fake, &mut slot, Move::Dispatch, None);
            let cost =
                self.prices
                    .cost(request.tier, request.body_bytes, request.max_output_tokens);
            let _ = self.act(&mut fake, &mut slot, Move::Settle { cost }, None);
        }
        for task in tasks.iter_mut().filter(|task| task.admitted && !task.ended) {
            let _ = self.run_task(&mut fake, task);
        }
    }

    fn largest_fit(&mut self, account: &Account) -> Option<Request> {
        let left = Room::of(account, PERIOD_END_MS)?.left();
        for tier in [Tier::Frontier, Tier::LowCostHosted] {
            let fits_at = |sim: &Sim<'_>, length: u64| {
                sim.body_bytes(tier, length, 1)
                    .map(|bytes| (bytes, sim.prices.hold(tier, bytes, 1)))
                    .filter(|(_, amount)| *amount <= left)
            };
            if fits_at(self, 1).is_none() {
                continue;
            }
            let (mut low, mut high) = (1, MOST_CONTENT);
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                if fits_at(self, middle).is_some() {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            let (body_bytes, amount) = fits_at(self, low)?;
            return Some(Request {
                tier,
                body_bytes,
                max_output_tokens: 1,
                amount,
            });
        }
        None
    }
}
