//! The person's count of hosted calls (r7 8.1L; the coordinator's log lines 16775, 17277 and 17330): ONE row of the
//! kind `model_count` of origin person for all agents together, which holds the count's text as halvo-route's
//! `Count::row` writes it and the instant that anchors its cap period. The four bodies that read and change it, and
//! nothing else, stand here: `model_room` and `set_model_cap` of the coordinator, `model_hold` and `model_settle` of
//! the runner. This module makes no figure of money and parses none from the count's text: every amount, cap and sum
//! comes from the functions of `halvo_route::count`. It makes two figures of time, the end of a cap period from the
//! anchor and the end of a hold's life from the request's instant, and its only time is the request's `Now`.

use halvo_route::count::{Count, HoldId, Usage};
use halvo_route::prices::Prices;
use halvo_route::spend::SpendCause;
use halvo_route::tier::Tier;
use halvo_schema::executable::body::{
    CapReply, ModelHold, ModelSettle, RoomReply, SetModelCap, Stored,
};
use halvo_schema::executable::names::{ModelTier, RoomState, YesNo};
use halvo_schema::executable::{CoordinatorReply, ModelCount, ModelRequestId, RunnerReply};
use halvo_schema::{ErrorName, Instant};

use crate::bind;
use crate::errors::{refused, Own};
use crate::host::{Host, Now};
use crate::rows::{self, record};

/// The life of a hold: it ends 120,000 ms behind the instant of the request that made it, past the 90 s that the door
/// of r3 U7.5 gives one request. A hold that nobody settles expires at its full amount at the first request at or
/// behind that end.
pub(crate) const HOLD_LIFE_MS: u64 = 120_000;

const DAY_MS: u64 = 86_400_000;
const NANOS_PER_MS: i64 = 1_000_000;

/// What stands of the count: no row; one row that reads, with its anchor; or a row that does not read, which
/// `Count::read` or the record's parser refuses, or more than one row.
enum Stands {
    None,
    Reads { anchor: Instant, count: Count },
    Unread,
}

fn stands(host: &dyn Host) -> Stands {
    let mut texts = Vec::new();
    host.rows()
        .for_each(rows::MODEL_COUNT, &mut |row| texts.push(row.to_owned()));
    match texts.as_slice() {
        [] => Stands::None,
        [text] => ModelCount::parse(text.as_bytes())
            .ok()
            .and_then(|row| {
                Count::read(&row.text()).ok().map(|count| Stands::Reads {
                    anchor: row.anchor,
                    count,
                })
            })
            .unwrap_or(Stands::Unread),
        _ => Stands::Unread,
    }
}

/// The error name of a refusal of the count's functions: `over_cap` for a hold that does not fit the room left,
/// `no_cap` for a hold under no cap, `repeat` for a hold of an id that is open already, `no_hold` for a settle of an id
/// that is not open, and `state` for every other cause.
fn spend(cause: SpendCause) -> ErrorName {
    let name = match cause {
        SpendCause::OverCap => "over_cap",
        SpendCause::NoCap => "no_cap",
        SpendCause::Repeat | SpendCause::OtherArguments => "repeat",
        SpendCause::NoHold => "no_hold",
        SpendCause::NotActive | SpendCause::Used | SpendCause::State => return ErrorName::State,
    };
    declared(name)
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).unwrap_or(ErrorName::Internal)
}

fn unread() -> ErrorName {
    declared("unread")
}

/// The milliseconds of an instant, `state` for an instant before 1970.
fn ms_of(at: Instant) -> Result<u64, ErrorName> {
    u64::try_from(at.0.div_euclid(NANOS_PER_MS)).map_err(|_| ErrorName::State)
}

/// The year, the month and the day of the month of a count of days since 1970-01-01, in the civil calendar of UTC
/// (H. Hinnant's algorithm); `days_of` is its inverse.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn days_of(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn month_days(year: i64, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

/// The instant `months` calendar months behind the anchor, in milliseconds: the anchor's day of the month and time of
/// day in UTC, at that month's last day where the month has no such day.
fn months_behind(anchor_ms: u64, months: u64) -> Option<u64> {
    let (year, month, day) = civil(i64::try_from(anchor_ms / DAY_MS).ok()?);
    let index = i64::from(month - 1).checked_add(i64::try_from(months).ok()?)?;
    let (year, month) = (year + index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
    let days = days_of(year, month, day.min(month_days(year, month)));
    u64::try_from(days)
        .ok()?
        .checked_mul(DAY_MS)?
        .checked_add(anchor_ms % DAY_MS)
}

/// The end of the cap period that holds `now_ms` (the coordinator's log line 16775, (1)): the first instant one or
/// more calendar months behind the anchor that lies after `now_ms`. A turn does not move the anchor.
fn period_end(anchor: Instant, now_ms: u64) -> Result<u64, ErrorName> {
    let anchor_ms = ms_of(anchor)?;
    let mut months = if now_ms <= anchor_ms {
        1
    } else {
        let (ya, ma, _) = civil(i64::try_from(anchor_ms / DAY_MS).map_err(|_| ErrorName::State)?);
        let (yn, mn, _) = civil(i64::try_from(now_ms / DAY_MS).map_err(|_| ErrorName::State)?);
        u64::try_from((yn - ya) * 12 + i64::from(mn) - i64::from(ma))
            .map_err(|_| ErrorName::State)?
            .max(1)
    };
    loop {
        let end = months_behind(anchor_ms, months).ok_or(ErrorName::State)?;
        if end > now_ms {
            return Ok(end);
        }
        months += 1;
    }
}

/// The end of the count's own cap period, as `Count::room` gives it; for a count with no cap, for which `room` gives
/// none, the room of a copy of the count that holds a cap, whose period is the count's.
fn own_end(count: &Count) -> Option<u64> {
    count
        .room()
        .or_else(|| count.set_cap(1).ok()?.room())
        .map(|room| room.period_end_ms)
}

/// The count at the request's instant, by the rules of the caller (the coordinator's log lines 16775, (3), and 17560):
/// before the end of the count's period it expires every hold whose life ended at or before the instant, at its full
/// amount. At or behind that end it expires first every hold whose life ended inside the old period, before its end;
/// it turns second, to the end of the period that holds the instant; and it expires third every hold whose life ended
/// since, into the new period. So every hold that expires counts in a period that a cap reads: in the other order a
/// hold whose life ended in the old period would be charged to the new one, and with one expiry before the turn a hold
/// whose life ended in the new period would be charged to the old one and cleared with it.
fn at_instant(anchor: Instant, count: &Count, now_ms: u64) -> Result<Count, ErrorName> {
    let end = own_end(count).ok_or(ErrorName::State)?;
    if now_ms < end {
        return count.expire(now_ms).map_err(spend);
    }
    count
        .expire(end.saturating_sub(1))
        .map_err(spend)?
        .turn(period_end(anchor, now_ms)?)
        .expire(now_ms)
        .map_err(spend)
}

/// A new count at nothing, under the build's default cap, anchored at the request's instant.
fn new_count(now: Now) -> Result<(Instant, Count), ErrorName> {
    Ok((now.at, Count::new(period_end(now.at, ms_of(now.at)?)?)))
}

/// Replaces the count's ONE row in one write, by the store's removal call of a kind of origin person: the row that
/// stands leaves and the new one is stored, never two rows of the kind and never none between. `length` where the
/// count's text needs more pieces than its row holds.
fn write(host: &mut dyn Host, anchor: Instant, count: &Count) -> Result<(), ErrorName> {
    let row = ModelCount::of(anchor, &count.row()).map_err(refused)?;
    let records = vec![record(rows::MODEL_COUNT, row.to_json())?];
    host.remove_person(rows::MODEL_COUNT, records)?;
    Ok(())
}

fn hold_id(request: &ModelRequestId) -> Result<HoldId, ErrorName> {
    let digits = request.as_str().as_bytes();
    let mut id = [0u8; 16];
    for (at, byte) in id.iter_mut().enumerate() {
        let pair =
            std::str::from_utf8(&digits[2 * at..2 * at + 2]).map_err(|_| ErrorName::Malformed)?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| ErrorName::Malformed)?;
    }
    Ok(HoldId(id))
}

fn tier(tier: ModelTier) -> Tier {
    match tier {
        ModelTier::OnDevice => Tier::OnDevice,
        ModelTier::LowCostHosted => Tier::LowCostHosted,
        ModelTier::Frontier => Tier::Frontier,
    }
}

fn stored_one() -> RunnerReply {
    RunnerReply::Stored(Stored { stored: 1 })
}

/// The rule of the way back (the coordinator's log lines 17342, point 5, and 17603, point 3): a hold is admitted only
/// for an agent whose newest set-up holds a hosted tier with its agreement, read by `rows::standing_agreement` alone,
/// and only for a tier whose triple that agreement holds, the triple of the tier in use; else `config`: the agent's
/// set-up, with its agreement, does not allow a model of that tier. So a person's way back reaches a live run at its
/// next request.
fn admitted(host: &dyn Host, b: &ModelHold) -> Result<(), ErrorName> {
    let holds = rows::standing_agreement(host.rows(), &b.agent)?
        .is_some_and(|agreement| agreement.hosts.iter().any(|hosted| hosted.tier == b.tier));
    if holds {
        Ok(())
    } else {
        Err(Own::Config.into())
    }
}

/// `model_hold`: after the checks of a runner body and the rule of the way back (`admitted`), the hold of the request
/// at the build's prices, whose life ends `HOLD_LIFE_MS` behind the request's instant, on the count at the request's
/// instant; where no row stands, on a new count anchored at the request's instant. `unread` for a row that does not
/// read; a refusal of `Count::reserve` by `spend`. A refused hold writes nothing.
pub(crate) fn hold(host: &mut dyn Host, now: Now, b: &ModelHold) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(&*host, &b.agent)?;
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    admitted(&*host, b)?;
    let now_ms = ms_of(now.at)?;
    let (anchor, count) = match stands(&*host) {
        Stands::None => new_count(now)?,
        Stands::Reads { anchor, count } => (anchor, count),
        Stands::Unread => return Err(unread()),
    };
    let ends_ms = now_ms.checked_add(HOLD_LIFE_MS).ok_or(ErrorName::State)?;
    let next = at_instant(anchor, &count, now_ms)?
        .reserve(
            hold_id(&b.request)?,
            Prices::release(),
            tier(b.tier),
            b.body_bytes,
            b.output_limit,
            ends_ms,
        )
        .map_err(spend)?;
    write(host, anchor, &next)?;
    Ok(stored_one())
}

/// `model_settle`: after the checks of a runner body, the settle of the request's hold at the usage that the host's
/// answer reported, or at nothing for a request of which nothing was sent, on the count at the request's instant.
/// `no_hold` where no row stands or the id is not open, a hold whose life ended before the settle among them; `unread`
/// for a row that does not read. A refused settle writes nothing.
pub(crate) fn settle(
    host: &mut dyn Host,
    now: Now,
    b: &ModelSettle,
) -> Result<RunnerReply, ErrorName> {
    bind::runner_agent(&*host, &b.agent)?;
    bind::own_run(&*host, now, &b.run, &b.agent)?;
    bind::open_run(&*host, &b.run)?;
    let now_ms = ms_of(now.at)?;
    let (anchor, count) = match stands(&*host) {
        Stands::None => return Err(spend(SpendCause::NoHold)),
        Stands::Reads { anchor, count } => (anchor, count),
        Stands::Unread => return Err(unread()),
    };
    let usage = match &b.usage {
        Some(usage) => Usage::Reported {
            tokens_in: usage.tokens_in,
            tokens_out: usage.tokens_out,
        },
        None => Usage::Unsent,
    };
    let next = at_instant(anchor, &count, now_ms)?
        .settle(hold_id(&b.request)?, Prices::release(), usage)
        .map_err(spend)?;
    write(host, anchor, &next)?;
    Ok(stored_one())
}

/// `model_room`: what the count is at the request's instant, by `Count::room` of the count at that instant; it stores
/// nothing. With no row, the cap of a new count and its account; with a row that does not read, no figure.
pub(crate) fn room(host: &dyn Host, now: Now) -> Result<CoordinatorReply, ErrorName> {
    let reply = match stands(host) {
        Stands::None => {
            let account = Count::new(0).account();
            RoomReply {
                room: RoomState::None,
                cap_minor: account.cap_minor,
                spent: Some(account.spent),
                held: Some(account.held),
                period_end_ms: None,
            }
        }
        Stands::Reads { anchor, count } => {
            let now_ms = ms_of(now.at)?;
            let count = at_instant(anchor, &count, now_ms)?;
            match count.room() {
                Some(room) => RoomReply {
                    room: RoomState::Reads,
                    cap_minor: Some(room.cap_minor),
                    spent: Some(room.spent),
                    held: Some(room.held),
                    period_end_ms: Some(room.period_end_ms),
                },
                None => {
                    let account = count.account();
                    RoomReply {
                        room: RoomState::Reads,
                        cap_minor: None,
                        spent: Some(account.spent),
                        held: Some(account.held),
                        period_end_ms: Some(period_end(anchor, now_ms)?),
                    }
                }
            }
        }
        Stands::Unread => RoomReply {
            room: RoomState::Unread,
            cap_minor: None,
            spent: None,
            held: None,
            period_end_ms: None,
        },
    };
    Ok(CoordinatorReply::Room(reply))
}

/// `set_model_cap`: the person's cap set by `Count::set_cap` on the count at the request's instant; where no row
/// stands or the row does not read, on a new count at nothing anchored at the request's instant, and the answer says
/// that the count is new. `length` for a cap outside the bounds of a cap line.
pub(crate) fn set_cap(
    host: &mut dyn Host,
    now: Now,
    b: &SetModelCap,
) -> Result<CoordinatorReply, ErrorName> {
    let now_ms = ms_of(now.at)?;
    let (anchor, count, new_count_word) = match stands(&*host) {
        Stands::Reads { anchor, count } => (anchor, at_instant(anchor, &count, now_ms)?, YesNo::No),
        Stands::None | Stands::Unread => {
            let (anchor, count) = new_count(now)?;
            (anchor, count, YesNo::Yes)
        }
    };
    let next = count
        .set_cap(u64::from(b.cap_minor))
        .map_err(|_| ErrorName::from(Own::Length))?;
    write(host, anchor, &next)?;
    Ok(CoordinatorReply::Cap(CapReply {
        cap_minor: b.cap_minor,
        new_count: new_count_word,
    }))
}

#[cfg(test)]
mod tests {
    //! r7 8.1L, the sentences (1), (2), (4), (5) and (6) of the unit: the four bodies of the person's count driven
    //! through the agent store's two handlers over the store in memory, so that they run with the library's unit tests
    //! (the coordinator's ruling of 2026-11-23, msg_4d5c628e91b7). Every expected figure comes from the functions of
    //! halvo-route that price a hold and a cost, so no test holds a rate of the build.

    use halvo_route::prices::{Prices, MICRO_PER_MINOR};
    use halvo_route::release;
    use halvo_route::tier::Tier;
    use halvo_schema::executable::{CoordinatorReply, RunnerReply};
    use halvo_schema::{ErrorName, Instant, RowRead, TypedRecord, Workflow};
    use serde_json::{json, Value};

    use crate::host::{Host, Now};
    use crate::memory::MemoryStore;

    /// The instant of every request of a test but those that name their own, 2026-11-12T16:13:20Z.
    const NOW: Now = Now {
        at: Instant(1_794_500_000_000_000_000),
    };

    fn at(nanos: i64) -> Now {
        Now { at: Instant(nanos) }
    }

    fn id(prefix: char, n: u32) -> String {
        format!("{prefix}{n:031x}")
    }

    fn agent(n: u32) -> String {
        id('4', n)
    }

    fn run(n: u32) -> String {
        id('5', n)
    }

    fn declared(name: &str) -> ErrorName {
        ErrorName::read(name).expect("a declared error name")
    }

    fn rows(store: &MemoryStore, kind: &str) -> Vec<String> {
        let mut out = Vec::new();
        store.for_each(kind, &mut |row| out.push(row.to_owned()));
        out
    }

    fn coordinator_at(
        store: &mut MemoryStore,
        now: Now,
        body: Value,
    ) -> Result<CoordinatorReply, ErrorName> {
        crate::coordinator(store, now, body.to_string().as_bytes())
    }

    fn runner_at(store: &mut MemoryStore, now: Now, body: Value) -> Result<RunnerReply, ErrorName> {
        crate::runner(store, now, body.to_string().as_bytes())
    }

    /// The person's `set_model` for the first agent of the first workflow with `tier` and the triples `hosts`, each a
    /// made-up origin and model word of its tier, or null for the way back.
    fn set_model(store: &mut MemoryStore, tier: &str, hosts: &[&str]) -> Result<Value, ErrorName> {
        let agreement: Option<Vec<Value>> = (!hosts.is_empty()).then(|| {
            hosts
                .iter()
                .map(|tier| json!({"tier": tier, "origin": "https://hosted-model.test", "model": format!("test-model-{tier}")}))
                .collect()
        });
        coordinator_json(
            store,
            NOW,
            json!({"op": "set_model", "workflow": id('3', 1), "agent": agent(1), "tier": tier, "agreement": agreement}),
        )
    }

    /// The first agent of an approved workflow of the category of a bill, with the person's yes up to `frontier` with
    /// a triple of each hosted tier, so that a hold of either tier is admitted (r3 U7.7), and its run 1 started at
    /// `NOW`, which every runner body of the run needs.
    fn store_with() -> MemoryStore {
        let mut store = unagreed();
        set_model(&mut store, "frontier", &["low_cost_hosted", "frontier"]).expect("the yes");
        store
    }

    /// The first agent of an approved workflow of the category of a bill, with no yes to a hosted model, and its run 1
    /// started at `NOW`.
    fn unagreed() -> MemoryStore {
        let mut store = MemoryStore::new();
        let workflow: Workflow = serde_json::from_value(json!({
            "v": 1, "id": id('3', 1), "state": "approved", "category": "utility_bill_payment", "site": null,
            "app": "com.example.browser", "trigger": "schedule",
            "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767}, "confidence": 87,
            "agent": null,
        }))
        .expect("a workflow record");
        store.put_workflow(workflow);
        let create = json!({"op": "create_agent", "workflow": id('3', 1), "agent": agent(1)});
        coordinator_at(&mut store, NOW, create).expect("create_agent");
        let start = json!({"op": "start_run", "workflow": id('3', 1), "agent": agent(1), "run": run(1), "mode": "compile"});
        coordinator_at(&mut store, NOW, start).expect("start_run");
        store
    }

    const NANOS_PER_S: i64 = 1_000_000_000;

    /// 2026-11-12T16:13:20Z, the instant of `NOW`, and the ends of its first two calendar months in UTC.
    const NOW_S: i64 = 1_794_500_000;
    const FIRST_END_S: i64 = 1_797_092_000;
    const SECOND_END_S: i64 = 1_799_770_400;

    fn at_s(s: i64) -> Now {
        at(s * NANOS_PER_S)
    }

    fn ms(s: i64) -> u64 {
        u64::try_from(s * 1_000).expect("an instant after 1970")
    }

    fn coordinator_json(
        store: &mut MemoryStore,
        now: Now,
        body: Value,
    ) -> Result<Value, ErrorName> {
        coordinator_at(store, now, body).map(|reply| {
            serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("JSON")
        })
    }

    fn runner_json(store: &mut MemoryStore, now: Now, body: Value) -> Result<Value, ErrorName> {
        runner_at(store, now, body).map(|reply| {
            serde_json::from_str(&reply.to_json().expect("the reply's JSON")).expect("JSON")
        })
    }

    fn room(store: &mut MemoryStore, now: Now) -> Value {
        coordinator_json(
            store,
            now,
            json!({"op": "model_room", "workflow": null, "agent": null}),
        )
        .expect("model_room")
    }

    fn set_cap(store: &mut MemoryStore, now: Now, cap_minor: u32) -> Result<Value, ErrorName> {
        coordinator_json(
            store,
            now,
            json!({"op": "set_model_cap", "workflow": null, "agent": null, "cap_minor": cap_minor}),
        )
    }

    fn hold(
        store: &mut MemoryStore,
        now: Now,
        request: u32,
        tier: &str,
        body_bytes: u64,
        output_limit: u64,
    ) -> Result<Value, ErrorName> {
        runner_json(
            store,
            now,
            json!({
                "op": "model_hold", "agent": agent(1), "run": run(1), "request": id('c', request),
                "tier": tier, "body_bytes": body_bytes, "output_limit": output_limit,
            }),
        )
    }

    fn settle(
        store: &mut MemoryStore,
        now: Now,
        request: u32,
        usage: Value,
    ) -> Result<Value, ErrorName> {
        runner_json(
            store,
            now,
            json!({"op": "model_settle", "agent": agent(1), "run": run(1), "request": id('c', request), "usage": usage}),
        )
    }

    fn count_rows(store: &MemoryStore) -> Vec<String> {
        rows(store, "model_count")
    }

    /// The figures of a `reads` answer: what is spent, what is held and the end of the period.
    fn figures(answer: &Value) -> (Value, Value, Value) {
        assert_eq!(answer["room"], "reads", "{answer}");
        (
            answer["spent"].clone(),
            answer["held"].clone(),
            answer["period_end_ms"].clone(),
        )
    }

    /// Sentence (1): a hold that does not fit the room left is refused with `over_cap`, and the row stays as it was. The
    /// person's cap holds one hold of the test's size and not two; the first hold is admitted and the second, which alone
    /// would fit the cap, is refused, and the stored row is the same text before and after it.
    #[test]
    fn a_hold_above_the_room_left_is_refused() {
        let mut store = store_with();
        let one = Prices::release().hold(Tier::Frontier, 10_000, 1_000);
        let cap: u32 = 40;
        let cap_micro = u64::from(cap) * MICRO_PER_MINOR;
        assert!(
            one <= cap_micro && 2 * one > cap_micro,
            "the test's fixture: one hold of {one} fits the cap of {cap_micro} and two do not"
        );
        assert_eq!(
            set_cap(&mut store, NOW, cap),
            Ok(json!({"cap_minor": cap, "new_count": "yes"}))
        );
        assert_eq!(
            hold(&mut store, NOW, 1, "frontier", 10_000, 1_000),
            Ok(json!({"stored": 1})),
            "the first hold"
        );
        let before = count_rows(&store);
        assert_eq!(
            hold(&mut store, NOW, 2, "frontier", 10_000, 1_000),
            Err(declared("over_cap")),
            "the second hold"
        );
        assert_eq!(before.len(), 1, "one count row");
        assert_eq!(count_rows(&store), before, "the row after the refused hold");
        assert_eq!(
            figures(&room(&mut store, NOW)),
            (json!(0), json!(one), json!(ms(FIRST_END_S)))
        );
    }

    /// Sentence (2): a hold that fits settles at the reported usage. With no row standing, the first hold starts a count
    /// under the build's default cap and holds `Prices::hold` of its tier; its settle with the usage that the host's answer
    /// reported leaves nothing held and `Prices::cost` of that tier spent.
    #[test]
    fn a_hold_that_fits_settles_at_the_reported_usage() {
        let mut store = store_with();
        let prices = Prices::release();
        let held = prices.hold(Tier::LowCostHosted, 20_000, 2_000);
        let cost = prices.cost(Tier::LowCostHosted, 5_000, 300);
        assert_eq!(
            hold(&mut store, NOW, 1, "low_cost_hosted", 20_000, 2_000),
            Ok(json!({"stored": 1}))
        );
        let open = room(&mut store, NOW);
        assert_eq!(
            open["cap_minor"],
            json!(release::cap().map(|cap| cap.amount)),
            "the build's default cap"
        );
        assert_eq!(
            figures(&open),
            (json!(0), json!(held), json!(ms(FIRST_END_S)))
        );
        assert_eq!(
            settle(
                &mut store,
                NOW,
                1,
                json!({"tokens_in": 5_000, "tokens_out": 300})
            ),
            Ok(json!({"stored": 1}))
        );
        assert_eq!(
            figures(&room(&mut store, NOW)),
            (json!(cost), json!(0), json!(ms(FIRST_END_S)))
        );
    }

    /// Sentence (4) (the coordinator's log line 16775, (3)): the handler expires first and turns second, and turns only at
    /// or behind the period's end. The count is anchored at `NOW`; a hold is made 200 s before the period's end, so its
    /// life of 120 s ends 80 s before it. One millisecond before the end the hold has expired at its full amount into the
    /// old period and nothing turned; at the end the period turns, and the new period starts at nothing, with nothing held.
    /// In the other order the expired hold would be charged to the new period.
    #[test]
    fn a_hold_that_ended_in_the_old_period_counts_there_and_the_new_period_starts_at_nothing() {
        let mut store = store_with();
        let amount = Prices::release().hold(Tier::Frontier, 10_000, 1_000);
        assert_eq!(
            set_cap(&mut store, NOW, 2_000),
            Ok(json!({"cap_minor": 2_000, "new_count": "yes"}))
        );
        assert_eq!(
            hold(
                &mut store,
                at_s(FIRST_END_S - 200),
                1,
                "frontier",
                10_000,
                1_000
            ),
            Ok(json!({"stored": 1}))
        );
        assert_eq!(
            figures(&room(&mut store, at(FIRST_END_S * NANOS_PER_S - 1_000_000))),
            (json!(amount), json!(0), json!(ms(FIRST_END_S))),
            "one millisecond before the end"
        );
        assert_eq!(
            figures(&room(&mut store, at_s(FIRST_END_S))),
            (json!(0), json!(0), json!(ms(SECOND_END_S))),
            "at the end"
        );
    }

    /// Round 1 (verifier-53's probe (c); the coordinator's log line 17560): every hold that expires counts in a period
    /// that a cap reads. A hold is made 60 s before the end of the count's period, so its life ends 60 s inside the
    /// new period, and no request stores the count between. 10 s behind the end the new period holds it open; 61 s
    /// behind the end it has expired at its full amount into the new period: the handler expires to the period's end,
    /// turns, and then expires to the request's instant.
    #[test]
    fn a_hold_whose_life_ends_in_the_new_period_counts_there() {
        let mut store = store_with();
        let amount = Prices::release().hold(Tier::Frontier, 10_000, 1_000);
        assert_eq!(
            set_cap(&mut store, NOW, 2_000),
            Ok(json!({"cap_minor": 2_000, "new_count": "yes"}))
        );
        assert_eq!(
            hold(
                &mut store,
                at_s(FIRST_END_S - 60),
                1,
                "frontier",
                10_000,
                1_000
            ),
            Ok(json!({"stored": 1}))
        );
        assert_eq!(
            figures(&room(&mut store, at_s(FIRST_END_S + 10))),
            (json!(0), json!(amount), json!(ms(SECOND_END_S))),
            "10 s behind the end"
        );
        assert_eq!(
            figures(&room(&mut store, at_s(FIRST_END_S + 61))),
            (json!(amount), json!(0), json!(ms(SECOND_END_S))),
            "61 s behind the end"
        );
    }

    /// The take-in (the coordinator's log lines 17342, point 5, and 17603, point 3): a hold is admitted only for an
    /// agent whose newest set-up holds a hosted tier with its agreement, and only for a tier whose triple that agreement
    /// holds, the triple of the tier in use, so that the way back reaches a live run at its next request. (i) An agent
    /// with no yes: a hold is refused with `config`. (ii) A yes up to `frontier` with the triple of `frontier` alone: a
    /// hold of `frontier` is admitted, a hold of `low_cost_hosted` is refused. (iii) Behind the way back: a hold is
    /// refused. After each refusal the count's row is the text that stood before it.
    #[test]
    fn a_hold_is_admitted_only_for_a_tier_whose_triple_the_agents_agreement_holds() {
        let mut store = unagreed();
        assert_eq!(
            set_cap(&mut store, NOW, 2_000),
            Ok(json!({"cap_minor": 2_000, "new_count": "yes"}))
        );
        let before = count_rows(&store);
        assert_eq!(
            hold(&mut store, NOW, 1, "frontier", 100, 100),
            Err(declared("config")),
            "(i) the hold of an agent with no yes"
        );
        assert_eq!(count_rows(&store), before, "(i) the row");
        assert!(set_model(&mut store, "frontier", &["frontier"]).is_ok());
        assert_eq!(
            hold(&mut store, NOW, 2, "frontier", 100, 100),
            Ok(json!({"stored": 1})),
            "(ii) the hold of frontier"
        );
        let before = count_rows(&store);
        assert_eq!(
            hold(&mut store, NOW, 3, "low_cost_hosted", 100, 100),
            Err(declared("config")),
            "(ii) the hold of low_cost_hosted"
        );
        assert_eq!(count_rows(&store), before, "(ii) the row");
        assert!(set_model(&mut store, "on_device", &[]).is_ok());
        assert_eq!(
            hold(&mut store, NOW, 4, "frontier", 100, 100),
            Err(declared("config")),
            "(iii) the hold behind the way back"
        );
        assert_eq!(count_rows(&store), before, "(iii) the row");
    }

    /// Sentence (5) (the coordinator's log line 16775, (1)): the period ends at the anchor's day of the month and time of
    /// day one calendar month later, in UTC, at that month's last day where the month has no such day, and a turn does not
    /// move the anchor. Anchored on 31 January 2027 at noon, the period ends on 28 February at noon; the turn there, stored
    /// by a set of the cap, gives 31 March at noon, and the turn at that end 30 April at noon.
    #[test]
    fn the_period_ends_one_calendar_month_behind_its_anchor() {
        const JAN_31: i64 = 1_801_396_800;
        const FEB_28: i64 = 1_803_816_000;
        const MAR_31: i64 = 1_806_494_400;
        const APR_30: i64 = 1_809_086_400;
        let mut store = MemoryStore::new();
        let end = |store: &mut MemoryStore, s: i64| room(store, at_s(s))["period_end_ms"].clone();
        assert_eq!(
            set_cap(&mut store, at_s(JAN_31), 500),
            Ok(json!({"cap_minor": 500, "new_count": "yes"}))
        );
        assert_eq!(end(&mut store, JAN_31), json!(ms(FEB_28)), "at the anchor");
        assert_eq!(
            room(&mut store, at(FEB_28 * NANOS_PER_S - 1_000_000))["period_end_ms"],
            json!(ms(FEB_28)),
            "one millisecond before the first end"
        );
        assert_eq!(
            set_cap(&mut store, at_s(FEB_28), 500),
            Ok(json!({"cap_minor": 500, "new_count": "no"}))
        );
        assert_eq!(
            end(&mut store, FEB_28),
            json!(ms(MAR_31)),
            "at the first end"
        );
        assert_eq!(
            end(&mut store, MAR_31),
            json!(ms(APR_30)),
            "at the second end"
        );
    }

    /// Sentence (6) (the coordinator's log line 16775, (4)): a row that does not read admits no hold, and the person's set
    /// of the cap starts a new count at nothing. A row of the kind is planted whose text `Count::read` refuses (its held
    /// figure is not the sum of its open holds). The hold is refused with `unread` and the row stays; the room answers the
    /// third state with no figure; the set of the cap answers that the count is new, and the count then reads with the new
    /// cap, nothing spent, nothing held and a period anchored at the set's instant.
    #[test]
    fn a_row_that_does_not_read_admits_no_hold_and_a_set_cap_starts_at_nothing() {
        let mut store = store_with();
        store
            .commit(vec![TypedRecord {
                kind: "model_count",
                json: r#"{"v":1,"anchor":1790000000000000000,"count":["halvo_count form=1 cap=default active=yes spent=0 held=5 period_end_ms=1792592000000\n"]}"#.to_owned(),
            }])
            .expect("the planted row is stored");
        let planted = count_rows(&store);
        assert_eq!(
            hold(&mut store, NOW, 1, "low_cost_hosted", 100, 100),
            Err(declared("unread"))
        );
        assert_eq!(
            count_rows(&store),
            planted,
            "the row after the refused hold"
        );
        assert_eq!(
            room(&mut store, NOW),
            json!({"room": "unread", "cap_minor": null, "spent": null, "held": null, "period_end_ms": null})
        );
        assert_eq!(
            set_cap(&mut store, NOW, 300),
            Ok(json!({"cap_minor": 300, "new_count": "yes"}))
        );
        assert_eq!(
            room(&mut store, NOW),
            json!({"room": "reads", "cap_minor": 300, "spent": 0, "held": 0, "period_end_ms": ms(FIRST_END_S)})
        );
        assert_eq!(count_rows(&store).len(), 1, "one count row");
        assert_eq!(NOW, at_s(NOW_S));
    }
}
