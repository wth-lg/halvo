use super::Cause;
use serde::Serialize;

/// The limits of I0.10: a count is a `u32` and a time a `u64`. Every function that reads a
/// limit takes `&Limits`, so that a test runs the code with the model's own values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Limits {
    /// Runs live at once.
    pub max_live_runs: u32,
    /// Starts that wait in the queue.
    pub queue_max: u32,
    /// Seconds after `issue` at which a confirmation lapses.
    pub wait_limit_s: u64,
    /// Milliseconds between the presenter's report and the move `arm`.
    pub arm_ms: u64,
    /// Seconds between a stop and the kill.
    pub cancel_grace_s: u64,
    /// `run_started` events of one agent on one day.
    pub starts_per_day: u32,
    /// Items that one run raises.
    pub items_per_run: u32,
    /// Failed runs in a row that set the hold `failures`.
    pub fails_to_pause: u32,
    /// Days after which a missed slot gets no catch-up run.
    pub catch_up_days: u32,
    /// Seconds that one `agents_next` call waits.
    pub next_wait_s: u64,
    /// Milliseconds inside which changes reach the page as one reply.
    pub coalesce_ms: u64,
    /// Seconds that the coordinator waits for the owner's acknowledgement.
    pub owner_wait_s: u64,
    /// Agents that the coordinator keeps.
    pub roster_max: u32,
    /// Records of one body.
    pub request_records: u32,
    /// Bytes of one body.
    pub request_bytes: u32,
    /// Runs that a card shows.
    pub history_runs: u32,
    /// Parameters of one setup.
    pub setup_params: u32,
    /// Candidates of one parameter.
    pub setup_candidates: u32,
}

const FIELDS: [&str; 18] = [
    "max_live_runs",
    "queue_max",
    "wait_limit_s",
    "arm_ms",
    "cancel_grace_s",
    "starts_per_day",
    "items_per_run",
    "fails_to_pause",
    "catch_up_days",
    "next_wait_s",
    "coalesce_ms",
    "owner_wait_s",
    "roster_max",
    "request_records",
    "request_bytes",
    "history_runs",
    "setup_params",
    "setup_candidates",
];

impl Limits {
    /// The product's values (I0.10); a value changes only by a Revision.
    pub const PRODUCT: Limits = Limits {
        max_live_runs: 1,
        queue_max: 4,
        wait_limit_s: 900,
        arm_ms: 500,
        cancel_grace_s: 5,
        starts_per_day: 6,
        items_per_run: 8,
        fails_to_pause: 3,
        catch_up_days: 7,
        next_wait_s: 25,
        coalesce_ms: 250,
        owner_wait_s: 5,
        roster_max: 256,
        request_records: 8,
        request_bytes: 4096,
        history_runs: 5,
        setup_params: 8,
        setup_candidates: 8,
    };

    /// Reads a JSON object with exactly the eighteen field names of I0.10, each once and a
    /// whole number that fits its type, as `formal/r4-coordinator/limits.json` holds them; any
    /// other text, a doubled key among it, is `malformed`.
    pub fn from_json(text: &str) -> Result<Limits, Cause> {
        let value = super::strict::parse(text)?;
        let object = value.as_object().ok_or(Cause::Malformed)?;
        if object.len() != FIELDS.len() || FIELDS.iter().any(|f| !object.contains_key(*f)) {
            return Err(Cause::Malformed);
        }
        let n = |key: &str| object[key].as_u64().ok_or(Cause::Malformed);
        let c = |key: &str| u32::try_from(n(key)?).map_err(|_| Cause::Malformed);
        Ok(Limits {
            max_live_runs: c("max_live_runs")?,
            queue_max: c("queue_max")?,
            wait_limit_s: n("wait_limit_s")?,
            arm_ms: n("arm_ms")?,
            cancel_grace_s: n("cancel_grace_s")?,
            starts_per_day: c("starts_per_day")?,
            items_per_run: c("items_per_run")?,
            fails_to_pause: c("fails_to_pause")?,
            catch_up_days: c("catch_up_days")?,
            next_wait_s: n("next_wait_s")?,
            coalesce_ms: n("coalesce_ms")?,
            owner_wait_s: n("owner_wait_s")?,
            roster_max: c("roster_max")?,
            request_records: c("request_records")?,
            request_bytes: c("request_bytes")?,
            history_runs: c("history_runs")?,
            setup_params: c("setup_params")?,
            setup_candidates: c("setup_candidates")?,
        })
    }
}
