use std::num::NonZeroU32;

use crate::ids::{AgentId, WorkflowId};
use crate::record::Instant;

use super::bound::{check_kept, KeptField};
use super::json::{self, each_once, from_json, list, object_type, opt};
use super::names::{
    ActionClass, Cause, CheckpointState, EffectState, LeftReason, LogEvent, Mode, ModelTier,
    Outcome, ReplyState, YesNo,
};
use super::step::{ExecutableStep, PLAYBOOK_STEPS_MAX};
use super::text::{
    ApprovalId, Digest, EffectId, Name, OwnText, OwnValue, ProviderId, RunId, SecretText,
    SessionData, WebOrigin, OWN_TEXT_MAX, V1,
};
use super::Refusal;

/// The most parts of the stored session of one origin.
pub const SESSION_PARTS_MAX: u32 = 64;

/// The most pieces of the text of the person's count in its row, `model_count`, each of at most `OWN_TEXT_MAX` bytes.
pub const COUNT_PIECES_MAX: usize = 16;

macro_rules! run_list {
    ($then:ident $($pass:tt)*) => {
        $then! {
            [$($pass)*]
            before_start: Option<u32> = "before_start" => opt(from_json),
            cut: Option<YesNo> = "cut" => opt(from_json),
            no_key: Option<u32> = "no_key" => opt(from_json),
            not_to_account: Option<u32> = "not_to_account" => opt(from_json),
            incomplete: Option<u32> = "incomplete" => opt(from_json),
            reply_to_other: Option<u32> = "reply_to_other" => opt(from_json),
            more_pages: Option<YesNo> = "more_pages" => opt(from_json),
            held_recipient: Option<u32> = "held_recipient" => opt(from_json),
            organized: Option<u32> = "organized" => opt(from_json),
            other_task: Option<u32> = "other_task" => opt(from_json),
            held_earlier: Option<u32> = "held_earlier" => opt(from_json),
            /// The entries, on the pages that the list call read, that the call left out because an earlier page of
            /// the same call had given their id. They stand in this count alone and outside the sum of the items that
            /// were left out, and no value of it says that a list was read whole.
            earlier_page: Option<u32> = "earlier_page" => opt(from_json),
        }
    };
}

pub(in crate::executable) use run_list;

macro_rules! agent_run {
    ([] $( $(#[$doc:meta])* $field:ident : $ty:ty = $key:literal => $read:expr ),* $(,)?) => {
        /// The keys of the run's list of `agent_run`, in their order after `model`: each a count or null, but `cut`
        /// and `more_pages`, each `yes`, `no` or null.
        pub const RUN_LIST_KEYS: &[&str] = &[$($key),*];

        object_type! {
            /// One finished run, kind `agent_run`. `prompts` is the sum of `handbacks` and `confirmations`, `unknown`
            /// is at most `effects`, and a run that is `completed` has no unknown effect. The key `mode` always
            /// stands: `compile`, `replay`, or null in the record that the handler of `end_run` stores, whose outcome
            /// is `failed`; a null mode beside the outcome of a `finish` is refused. The keys of `RUN_LIST_KEYS`
            /// follow `model`, each always present with its value or null and held to its type alone.
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub struct AgentRun {
                pub v: V1 = "v" => from_json,
                pub workflow: WorkflowId = "workflow" => json::workflow_id,
                pub agent: AgentId = "agent" => json::agent_id,
                pub run: RunId = "run" => from_json,
                pub day: u32 = "day" => from_json,
                pub mode: Option<Mode> = "mode" => opt(from_json),
                pub outcome: Outcome = "outcome" => from_json,
                pub playbook: Option<NonZeroU32> = "playbook" => opt(from_json),
                pub steps: u32 = "steps" => from_json,
                pub prompts: u32 = "prompts" => from_json,
                pub handbacks: u32 = "handbacks" => from_json,
                pub confirmations: u32 = "confirmations" => from_json,
                pub effects: u32 = "effects" => from_json,
                pub unknown: u32 = "unknown" => from_json,
                pub model_calls: u32 = "model_calls" => from_json,
                pub tokens_in: u32 = "tokens_in" => from_json,
                pub tokens_out: u32 = "tokens_out" => from_json,
                pub wall_s: u32 = "wall_s" => from_json,
                pub model: Option<Name> = "model" => opt(from_json),
                $( $(#[$doc])* pub $field: $ty = $key => $read, )*
            }
            rules(r) {
                let fail = |key, rule| Err(Refusal::malformed("AgentRun", rule).at(key));
                if r.mode.is_none() && r.outcome != Outcome::Failed {
                    return fail("mode", "mode_null_after_finish");
                }
                if u64::from(r.prompts) != u64::from(r.handbacks) + u64::from(r.confirmations) {
                    return fail("prompts", "prompts_sum");
                }
                if r.unknown > r.effects {
                    return fail("unknown", "unknown_above_effects");
                }
                if r.outcome == Outcome::Completed && r.unknown > 0 {
                    return fail("unknown", "completed_with_unknown");
                }
                Ok(())
            }
        }

        impl AgentRun {
            /// The one maker of the stored `agent_run` record (A7.4), which a tool or a test of another crate calls
            /// in place of a record written by hand: the ids, the day, the mode and the outcome that the caller
            /// gives, every count 0, and `playbook`, `model` and every key of `RUN_LIST_KEYS` null until the caller
            /// sets its field. `to_json` writes its text in the order of A7.4; a caller that needs a JSON value
            /// parses that text. `to_json` refuses a record that breaks one of the four rules of its type; a
            /// refusal test parses a made text and changes its keys one at a time.
            pub fn new(
                workflow: WorkflowId,
                agent: AgentId,
                run: RunId,
                day: u32,
                mode: Option<Mode>,
                outcome: Outcome,
            ) -> AgentRun {
                AgentRun {
                    v: V1,
                    workflow,
                    agent,
                    run,
                    day,
                    mode,
                    outcome,
                    playbook: None,
                    steps: 0,
                    prompts: 0,
                    handbacks: 0,
                    confirmations: 0,
                    effects: 0,
                    unknown: 0,
                    model_calls: 0,
                    tokens_in: 0,
                    tokens_out: 0,
                    wall_s: 0,
                    model: None,
                    $( $field: None, )*
                }
            }
        }
    };
}

run_list!(agent_run);

object_type! {
    /// One step of one playbook, kind `agent_playbook`: the playbook's number, the step's number and the step,
    /// whose `n` is the row's `n`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentPlaybook {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub number: NonZeroU32 = "number" => from_json,
        pub n: NonZeroU32 = "n" => from_json,
        pub step: ExecutableStep = "step" => ExecutableStep::from_raw,
    }
    rules(p) {
        if p.step.n != p.n {
            return Err(Refusal::malformed("AgentPlaybook", "n_of_step").at("n"));
        }
        Ok(())
    }
}

object_type! {
    /// A slot of the bindings: its name and the value that the user typed, chose or confirmed at activation.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Slot {
        pub name: Name = "name" => from_json,
        pub value: OwnValue = "value" => from_json,
    }
    rules(_s) { Ok(()) }
}

pub(in crate::executable) fn slot_names_once(
    type_name: &'static str,
    slots: &[Slot],
) -> Result<(), Refusal> {
    let names: Vec<&Name> = slots.iter().map(|s| &s.name).collect();
    each_once(type_name, &names).map_err(|r| r.at("slots"))
}

pub(in crate::executable) fn origins_and_slots(
    type_name: &'static str,
    origins: &[WebOrigin],
    slots: &[Slot],
) -> Result<(), Refusal> {
    each_once(type_name, origins).map_err(|r| r.at("origins"))?;
    slot_names_once(type_name, slots)
}

object_type! {
    /// One revision of an agent's bindings, kind `agent_bindings`: its origins, each once, and its slots, each name
    /// once.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentBindings {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub origins: Vec<WebOrigin> = "origins" => list(from_json),
        pub slots: Vec<Slot> = "slots" => list(Slot::from_raw),
    }
    rules(b) { origins_and_slots("AgentBindings", &b.origins, &b.slots) }
}

object_type! {
    /// One state of one effect, kind `agent_effect`. Its class makes an effect; `kept` holds the kept fields of the
    /// class in their order, an amount of at most 32 bytes and an address of at most 320; `fields` is the number of
    /// the bound fields of the class. A `held`, `cleared` or `refused` row holds no approval and an `approved` row
    /// one; a row with the cause `ended` is `rejected`, and one with `reproposed` is `rejected` with no approval; a
    /// `not_made` row holds the cause `not_sent`, `not_replaced`, `unauthorized` or `throttled`, and no other row
    /// holds one of the four; no row holds `not_selected` or `no_start`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentEffect {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub effect: EffectId = "effect" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub state: EffectState = "state" => from_json,
        pub class: ActionClass = "class" => from_json,
        pub digest: Digest = "digest" => from_json,
        pub kept: Vec<KeptField> = "kept" => list(KeptField::from_raw),
        pub fields: u32 = "fields" => from_json,
        pub approval: Option<ApprovalId> = "approval" => opt(from_json),
        pub cause: Option<Cause> = "cause" => opt(from_json),
    }
    rules(e) {
        let fail = |key, rule| Err(Refusal::malformed("AgentEffect", rule).at(key));
        let count = e.class.bound_fields().len();
        if count == 0 {
            return fail("class", "class_without_effect");
        }
        check_kept(e.class, &e.kept).map_err(|r| r.at("kept"))?;
        if usize::try_from(e.fields).ok() != Some(count) {
            return fail("fields", "fields_of_class");
        }
        let approval = match e.state {
            EffectState::Held | EffectState::Cleared | EffectState::Refused => Some(false),
            EffectState::Approved => Some(true),
            _ => None,
        };
        if approval.is_some_and(|holds| holds != e.approval.is_some()) {
            return fail("approval", "approval_of_state");
        }
        if e.cause == Some(Cause::Ended) && e.state != EffectState::Rejected {
            return fail("cause", "ended_not_rejected");
        }
        let not_made = matches!(
            e.cause,
            Some(Cause::NotSent | Cause::NotReplaced | Cause::Unauthorized | Cause::Throttled)
        );
        let reproposed = e.cause == Some(Cause::Reproposed);
        if not_made != (e.state == EffectState::NotMade)
            || matches!(e.cause, Some(Cause::NotSelected | Cause::NoStart))
            || (reproposed && (e.state != EffectState::Rejected || e.approval.is_some()))
        {
            return fail("cause", "cause_of_state");
        }
        Ok(())
    }
}

/// The causes of the `stopped` line of the read of the page after the reply of a committed `payment` or `purchase`,
/// the line that names its effect (r3 U5.10-P): the page names the kept amount alone, another amount, no amount, or
/// could not be read.
pub const PAGE_CAUSES: [Cause; 4] = [
    Cause::AmountMatched,
    Cause::AmountDiffers,
    Cause::AmountUnread,
    Cause::PageUnread,
];

object_type! {
    /// One line of a run's log, kind `agent_log`: ids, an event, a step number, a cause, the model token and
    /// figures, and no text. A `start_accepted` line has `seq` 1 and no step, cause, model or figure; the cause
    /// `unauthorized` stands in a `step_failed` line alone, and `not_selected` and `no_start` in a `stopped` line
    /// alone. `page_amount` is the first amount that the page after a made payment or purchase named that is not
    /// the amount of the card, in minor units (`13.34` is 1334), as the runner read it (r3 U5.10-B): it may be a fee
    /// or a balance and is no charge that the record knows of; it stands in a `stopped` line with the cause
    /// `amount_differs` alone, and is null in every other line. `effect` names the effect whose page after the reply
    /// the line tells of (r3 U5.10-P; the coordinator's rulings of log lines 20487 and 20577): it stands in a
    /// `stopped` line with a cause of `PAGE_CAUSES` alone, and is missing from every other line; `null` under it is
    /// refused, and a line of `amount_matched` holds it always. A line of a build before the key stood has no such key,
    /// also where it tells of a page: it reads with none, ties to no effect, and keeps its text, since the key is
    /// written only where it holds an id.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentLog {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub event: LogEvent = "event" => from_json,
        pub step: Option<NonZeroU32> = "step" => opt(from_json),
        pub cause: Option<Cause> = "cause" => opt(from_json),
        pub model: Option<Name> = "model" => opt(from_json),
        pub ms: Option<u32> = "ms" => opt(from_json),
        pub tokens_in: Option<u32> = "tokens_in" => opt(from_json),
        pub tokens_out: Option<u32> = "tokens_out" => opt(from_json),
        pub page_amount: Option<u32> = "page_amount" => opt(from_json),
    }
    absent {
        pub effect: Option<EffectId> = "effect" => from_json,
    }
    rules(l) {
        let fail = |key, rule| Err(Refusal::malformed("AgentLog", rule).at(key));
        if l.page_amount.is_some()
            && (l.event != LogEvent::Stopped || l.cause != Some(Cause::AmountDiffers))
        {
            return fail("page_amount", "page_amount_of_cause");
        }
        if l.effect.is_some()
            && (l.event != LogEvent::Stopped || !l.cause.is_some_and(|c| PAGE_CAUSES.contains(&c)))
        {
            return fail("effect", "effect_of_cause");
        }
        if l.cause == Some(Cause::AmountMatched) && l.effect.is_none() {
            return fail("effect", "amount_matched_without_effect");
        }
        if l.event == LogEvent::StartAccepted {
            if l.seq.get() != 1 {
                return fail("seq", "start_accepted_first");
            }
            let held = [
                ("step", l.step.is_some()),
                ("cause", l.cause.is_some()),
                ("model", l.model.is_some()),
                ("ms", l.ms.is_some()),
                ("tokens_in", l.tokens_in.is_some()),
                ("tokens_out", l.tokens_out.is_some()),
            ];
            if let Some((key, _)) = held.iter().find(|(_, held)| *held) {
                return fail(key, "start_accepted_form");
            }
        }
        let event = match l.cause {
            Some(Cause::Unauthorized) => Some(LogEvent::StepFailed),
            Some(Cause::NotSelected | Cause::NoStart) => Some(LogEvent::Stopped),
            _ => None,
        };
        if event.is_some_and(|event| event != l.event) {
            return fail("cause", "cause_of_event");
        }
        Ok(())
    }
}

object_type! {
    /// One state of the reply to one message, kind `agent_reply`: ids, the message's id, the draft's id, the effect of
    /// the send, the state, the day and an instant, and no address, no subject and no text of a message. Every state
    /// but `lost` holds the draft's id, and `lost` none; `begun`, `committed` and `unknown` name the effect of the
    /// send, `lost` and `withheld` none, and `drafted` names one only after `begun`. Which row may follow which is
    /// the store's to hold.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentReply {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub message: ProviderId = "message" => from_json,
        pub draft: Option<ProviderId> = "draft" => opt(from_json),
        pub effect: Option<EffectId> = "effect" => opt(from_json),
        pub state: ReplyState = "state" => from_json,
    }
    rules(r) {
        let fail = |key, rule| Err(Refusal::malformed("AgentReply", rule).at(key));
        if r.draft.is_none() != (r.state == ReplyState::Lost) {
            return fail("draft", "draft_of_state");
        }
        let effect = match r.state {
            ReplyState::Begun | ReplyState::Committed | ReplyState::Unknown => Some(true),
            ReplyState::Lost | ReplyState::Withheld => Some(false),
            ReplyState::Drafted => None,
        };
        if effect.is_some_and(|holds| holds != r.effect.is_some()) {
            return fail("effect", "effect_of_state");
        }
        Ok(())
    }
}

/// The most bytes of the JSON text of a kept reply text, its pieces of `OwnValue` as a list (r3 U7.9): the bound of a
/// body that the writer model's text may give (crates/halvo-runner/src/model/write.rs, `BODY_JSON_MAX`).
pub const REPLY_BODY_JSON_MAX: usize = 3_072;

/// Refuses a kept reply text that is empty or whose JSON text passes `REPLY_BODY_JSON_MAX` bytes (r3 U7.9).
pub(in crate::executable) fn reply_body_fits(
    type_name: &'static str,
    body: &OwnValue,
) -> Result<(), Refusal> {
    if body.is_empty() {
        return Err(Refusal::malformed(type_name, "body_empty").at("body"));
    }
    let bytes = serde_json::to_string(body).map_or(usize::MAX, |text| text.len());
    if bytes > REPLY_BODY_JSON_MAX {
        return Err(Refusal::length(type_name, "body_json_max").at("body"));
    }
    Ok(())
}

object_type! {
    /// The text of a reply that a writer model wrote, kept beside its reply row for 35 days (r3 U7.9), kind
    /// `agent_reply_body`: ids, the day and the instant of the request that stored the first `drafted` row of the
    /// draft, the message's id, the draft's id, and under `body` the text as the draft step cut it and prepared it for
    /// the draft, in the pieces of `OwnValue`, never empty and its JSON at most `REPLY_BODY_JSON_MAX` bytes. No
    /// address, no subject and no text of a message. The handler of `reply_row` stores it in the commit of that
    /// `drafted` row, and the handler of `load_reply_body` hands it to the agent's own run alone while the agent's
    /// newest row of the message is `drafted` with that draft.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentReplyBody {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub message: ProviderId = "message" => from_json,
        pub draft: ProviderId = "draft" => from_json,
        pub body: OwnValue = "body" => from_json,
    }
    rules(r) { reply_body_fits("AgentReplyBody", &r.body) }
}

object_type! {
    /// The instant at which an agent was set up, kind `agent_start`: ids, the day and the instant, one row for an
    /// agent, which A7.4 gives the handler of `create_agent` to store once.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentStart {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
    }
    rules(_s) { Ok(()) }
}

object_type! {
    /// The fact that a compile finished one playbook number of an agent, kind `agent_finished`: ids, the number, its
    /// count of steps, 1 to 64, and the id of the run whose record of the mode `compile` with the outcome `completed`
    /// the handler of `finish` stored in the same commit (A7.4; CP-182).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentFinished {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub number: NonZeroU32 = "number" => from_json,
        pub steps: NonZeroU32 = "steps" => from_json,
        pub run: RunId = "run" => from_json,
    }
    rules(f) {
        if usize::try_from(f.steps.get()).map_or(true, |n| n > PLAYBOOK_STEPS_MAX) {
            return Err(Refusal::length("AgentFinished", "steps").at("steps"));
        }
        Ok(())
    }
}

/// An origin or a model word of a `Hosted`, read from a JSON string: 1 to `OWN_TEXT_MAX` bytes, kept as given and never
/// trimmed. An empty one is `malformed` and a longer one `length`.
fn hosted_word(raw: &serde_json::value::RawValue) -> Result<OwnText, Refusal> {
    let word = json::string("Hosted", raw)?;
    if word.is_empty() {
        return Err(Refusal::malformed("Hosted", "empty_word"));
    }
    if word.len() > OWN_TEXT_MAX {
        return Err(Refusal::length("Hosted", "word_length"));
    }
    OwnText::parse(&word)
}

object_type! {
    /// One hosted tier of a person's yes to a hosted model (r3 U7.7): a hosted tier, the origin of its host and the
    /// model word of its requests, as the caller read them from the router's table when it asked. The tier is not
    /// `on_device`; the origin and the model word are 1 to `OWN_TEXT_MAX` bytes each, stored as given and never
    /// trimmed. The type compares no word with a table.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Hosted {
        pub tier: ModelTier = "tier" => from_json,
        pub origin: OwnText = "origin" => hosted_word,
        pub model: OwnText = "model" => hosted_word,
    }
    rules(h) {
        if h.tier == ModelTier::OnDevice {
            return Err(Refusal::malformed("Hosted", "on_device").at("tier"));
        }
        Ok(())
    }
}

/// The rules of a list of `Hosted` under the ceiling tier `ceiling` (r3 U7.7): each tier stands once, the list holds a
/// triple of the ceiling itself, and no tier above it. With the rule of `Hosted` that no tier is `on_device`, a list
/// holds at most two triples. A ceiling `on_device` passes no list.
pub(in crate::executable) fn hosts_up_to(
    type_name: &'static str,
    ceiling: ModelTier,
    hosts: &[Hosted],
) -> Result<(), Refusal> {
    let tiers: Vec<ModelTier> = hosts.iter().map(|h| h.tier).collect();
    each_once(type_name, &tiers)?;
    if !tiers.contains(&ceiling) {
        return Err(Refusal::malformed(type_name, "no_triple_of_the_ceiling"));
    }
    if tiers.iter().any(|tier| *tier > ceiling) {
        return Err(Refusal::malformed(type_name, "tier_above_the_ceiling"));
    }
    Ok(())
}

object_type! {
    /// The person's yes for ONE agent to a hosted model, kind `agent_agreement` (r3 U7.7): ids, the ceiling tier of
    /// the agent, the `Hosted` triples as the caller handed them (`hosts`): one of the ceiling, none of a tier above
    /// it, at most one of each tier, and no rule for a missing lower tier; and the instant of the yes, the request's.
    /// At most one row stands for an agent: the coordinator body `set_model` writes it in one write with the new
    /// revision of the agent's set-up and removes it at the way back.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentAgreement {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub tier: ModelTier = "tier" => from_json,
        pub hosts: Vec<Hosted> = "hosts" => list(Hosted::from_raw),
        pub at: Instant = "at" => json::instant,
    }
    rules(a) { hosts_up_to("AgentAgreement", a.tier, &a.hosts).map_err(|r| r.at("hosts")) }
}

object_type! {
    /// The person's key for a model's host, kind `model_key` (r3 U7.6): ONE row for the person and none for an agent,
    /// which names no workflow. The key is at least one byte and at most `OWN_TEXT_MAX` bytes, taken as given and never
    /// trimmed; `Debug` prints no part of it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelKey {
        pub v: V1 = "v" => from_json,
        pub key: SecretText = "key" => from_json,
    }
    rules(r) {
        if r.key.expose().len() > OWN_TEXT_MAX {
            return Err(Refusal::length("ModelKey", "key_length").at("key"));
        }
        Ok(())
    }
}

object_type! {
    /// The person's count of hosted calls, kind `model_count` (r7 8.1L): ONE row for the person and all agents
    /// together, which names no workflow and no agent. `anchor` is the instant that anchors the count's cap period;
    /// `count` is the count's text as halvo-route's `Count::row` writes it, in 1 to `COUNT_PIECES_MAX` pieces of 1 to
    /// `OWN_TEXT_MAX` bytes, whose joined text is the count's. This type reads no figure of the text.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ModelCount {
        pub v: V1 = "v" => from_json,
        pub anchor: Instant = "anchor" => json::instant,
        pub count: Vec<OwnText> = "count" => list(from_json),
    }
    rules(r) {
        if r.count.is_empty() || r.count.iter().any(|piece| piece.as_str().is_empty()) {
            return Err(Refusal::malformed("ModelCount", "count_piece").at("count"));
        }
        if r.count.len() > COUNT_PIECES_MAX {
            return Err(Refusal::length("ModelCount", "count_pieces").at("count"));
        }
        Ok(())
    }
}

impl ModelCount {
    /// The row of the count's text `text` with the anchor `anchor`: the text cut into pieces of at most `OWN_TEXT_MAX`
    /// bytes, each at a boundary of a character; the refusal `count_piece` for an empty text and `count_pieces` for a
    /// text that needs more than `COUNT_PIECES_MAX` pieces.
    pub fn of(anchor: Instant, text: &str) -> Result<ModelCount, Refusal> {
        let mut count = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let mut end = rest.len().min(OWN_TEXT_MAX);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            count.push(OwnText::parse(&rest[..end])?);
            rest = &rest[end..];
        }
        let row = ModelCount {
            v: V1,
            anchor,
            count,
        };
        row.rules()?;
        Ok(row)
    }

    /// The count's text: its pieces joined.
    pub fn text(&self) -> String {
        self.count.iter().map(OwnText::as_str).collect()
    }
}

pub(in crate::executable) fn part_of(
    type_name: &'static str,
    part: NonZeroU32,
    of: NonZeroU32,
    data: Option<&SessionData>,
) -> Result<(), Refusal> {
    if of.get() > SESSION_PARTS_MAX {
        return Err(Refusal::malformed(type_name, "parts_max").at("of"));
    }
    if part > of {
        return Err(Refusal::malformed(type_name, "part_above_of").at("part"));
    }
    if data.is_some_and(|d| part < of && !d.is_full()) {
        return Err(Refusal::malformed(type_name, "part_not_full").at("data"));
    }
    Ok(())
}

object_type! {
    /// One part of the stored session of one origin, kind `agent_session`: part `part` of `of`, at most 64, whose
    /// data is base64 text in pieces of 1,024 bytes, at most 48; every part but the last holds 48 full pieces.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentSession {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub origin: WebOrigin = "origin" => from_json,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub part: NonZeroU32 = "part" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
        pub day: u32 = "day" => from_json,
        pub data: SessionData = "data" => from_json,
    }
    rules(s) { part_of("AgentSession", s.part, s.of, Some(&s.data)) }
}

pub(in crate::executable) fn checkpoint_copy(
    type_name: &'static str,
    paths: [(&'static str, &OwnValue); 3],
    state: CheckpointState,
    sha256: Option<&Digest>,
    bytes: Option<u32>,
) -> Result<(), Refusal> {
    let fail = |key, rule| Err(Refusal::malformed(type_name, rule).at(key));
    if let Some((key, _)) = paths.iter().find(|(_, value)| value.is_empty()) {
        return fail(key, "empty_path");
    }
    if sha256.is_some() != bytes.is_some() {
        return fail("bytes", "digest_and_bytes");
    }
    match (state, sha256) {
        (CheckpointState::Planned, Some(_)) => fail("sha256", "planned_with_digest"),
        (CheckpointState::Made, None) => fail("sha256", "made_without_digest"),
        _ => Ok(()),
    }
}

object_type! {
    /// One state of one checkpoint that `prepare` made beside a workbook, kind `agent_checkpoint`: the folder that
    /// `prepare` was given, the workbook's path as given to it and the copy's file name, each a value of at least one
    /// byte in pieces of at most 1,024 bytes, the copy's SHA-256 and length, both or neither, and the state. The row
    /// with `seq` 1 is `planned` and holds no digest, and a `planned` row has `seq` 1; a `made` row has `seq` 2 and a
    /// digest; a `gone` or `left` row has `seq` 2 or 3, since a checkpoint has at most three rows (which of the two
    /// follows which newest row is the store's to hold); `left` names the reason in a `left` row and is null in every
    /// other.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentCheckpoint {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub run: RunId = "run" => from_json,
        pub seq: NonZeroU32 = "seq" => from_json,
        pub day: u32 = "day" => from_json,
        pub at: Instant = "at" => json::instant,
        pub folder: OwnValue = "folder" => from_json,
        pub workbook: OwnValue = "workbook" => from_json,
        pub copy: OwnValue = "copy" => from_json,
        pub sha256: Option<Digest> = "sha256" => opt(from_json),
        pub bytes: Option<u32> = "bytes" => opt(from_json),
        pub state: CheckpointState = "state" => from_json,
        pub left: Option<LeftReason> = "left" => opt(from_json),
    }
    rules(c) {
        let fail = |key, rule| Err(Refusal::malformed("AgentCheckpoint", rule).at(key));
        let first = c.seq.get() == 1;
        if first != (c.state == CheckpointState::Planned)
            || (c.state == CheckpointState::Made && c.seq.get() != 2)
            || c.seq.get() > 3
        {
            return fail("seq", "seq_of_state");
        }
        checkpoint_copy(
            "AgentCheckpoint",
            [("folder", &c.folder), ("workbook", &c.workbook), ("copy", &c.copy)],
            c.state,
            c.sha256.as_ref(),
            c.bytes,
        )?;
        if c.left.is_some() != (c.state == CheckpointState::Left) {
            return fail("left", "left_of_state");
        }
        Ok(())
    }
}
