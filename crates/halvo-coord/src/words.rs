//! The words table (I1.1): every sentence that the interface shows, each whole, with named
//! placeholders in braces. The core composes every text from it, and the page formats nothing.

use crate::types::Cause;
use std::collections::BTreeMap;

/// The table: a key and an English sentence per entry, one entry per name of the key families
/// of I1.1. The placeholders of `line.run.<end>`: none.
pub const KEYS: &[(&str, &str)] = &[
    ("status.needs_you", "Needs you"),
    ("status.problem", "Problem"),
    ("status.working", "Working"),
    ("status.waiting_turn", "Waiting its turn"),
    ("status.all_set", "All set"),
    ("status.off", "Off"),
    ("status.not_yet", "Not yet"),
    ("say.needs_you", "{name} needs you."),
    ("say.problem", "{name} has a problem."),
    ("say.working", "{name} is working."),
    ("say.waiting_turn", "{name} is waiting its turn."),
    ("say.all_set", "{name} is all set."),
    ("say.off", "{name} is off."),
    ("say.not_yet", "{name} cannot run yet."),
    (
        "line.needs_you.confirm",
        "It wants your answer before it goes on.",
    ),
    ("line.needs_you.sign_in", "It needs you to sign in."),
    (
        "line.needs_you.link_account",
        "It needs you to link your account.",
    ),
    (
        "line.needs_you.stuck",
        "It is stuck on a step and needs your help.",
    ),
    (
        "line.needs_you.check",
        "Tell it whether its last task went through.",
    ),
    (
        "line.needs_you.setup",
        "It needs a few answers before it can start.",
    ),
    ("line.needs_you.ready", "It is ready to start when you are."),
    (
        "line.needs_you.show",
        "It needs you to show it the task once.",
    ),
    (
        "line.not_yet.category",
        "Halvo cannot do this kind of task yet.",
    ),
    ("line.off.person", "You switched it off."),
    (
        "line.problem.failures",
        "It was switched off because it failed several times in a row.",
    ),
    ("line.working.starting", "It is starting."),
    ("line.working.working", "It is doing the task now."),
    ("line.working.stopping", "It is stopping."),
    (
        "line.waiting_turn.queued",
        "It will start when the agent before it is done.",
    ),
    ("line.problem.failed", "Its last run did not finish."),
    (
        "line.problem.limit",
        "Its last run went on too long and was stopped.",
    ),
    (
        "line.problem.class_mismatch",
        "Its last run tried something it may not do.",
    ),
    (
        "line.problem.refused",
        "Halvo stopped a step of its last run.",
    ),
    (
        "line.problem.budget",
        "Its last run asked you too many times.",
    ),
    (
        "line.problem.kind",
        "Its last run asked for something Halvo does not allow.",
    ),
    (
        "line.all_set.tried_today",
        "It has tried as often as it may today.",
    ),
    ("line.all_set.new", "It has not run yet."),
    ("line.all_set.done", "Its last run went well."),
    ("line.all_set.nothing", "Its last run found nothing to do."),
    ("line.all_set.stopped_early", "Its last run stopped early."),
    ("line.run.completed", "It finished the task."),
    ("line.run.nothing_to_do", "It found nothing to do."),
    ("line.run.declined", "You said no, so it stopped."),
    ("line.run.handed_back", "It handed the task back to you."),
    (
        "line.run.class_mismatch",
        "It stopped before doing something it may not do.",
    ),
    ("line.run.refused", "Halvo stopped one of its steps."),
    ("line.run.limit", "It went on too long and was stopped."),
    (
        "line.run.effect_unknown",
        "It could not tell whether its last step went through.",
    ),
    ("line.run.failed", "It could not finish."),
    ("line.run.cancelled", "You stopped it."),
    (
        "line.run.lapsed",
        "It stopped because no answer came in time.",
    ),
    ("line.run.interrupted", "It stopped when Halvo closed."),
    (
        "line.run.deleted",
        "It stopped because its task was removed.",
    ),
    (
        "line.run.budget",
        "It stopped after asking you too many times.",
    ),
    (
        "line.run.kind",
        "It stopped after asking for something Halvo does not allow.",
    ),
    ("line.level.low", "Quick"),
    ("line.level.medium", "Standard"),
    ("line.level.high", "Thorough"),
    ("line.runs_on.on_device", "Runs on this computer."),
    (
        "line.asks.observe",
        "It only watches and does nothing for you.",
    ),
    (
        "line.asks.draft_and_confirm",
        "It asks you before it sends, pays or signs anything.",
    ),
    ("line.asks.unattended", "It works without asking you first."),
    ("line.time", "It stops after {minutes} minutes."),
    ("line.files", "It can open the files that you choose."),
    ("category.utility_bill_payment", "Paying a utility bill"),
    ("category.tenant_rent", "Handling a tenant's rent"),
    (
        "category.lease_renewal_or_change",
        "Renewing or changing a lease",
    ),
    (
        "category.tenant_application_review",
        "Reviewing a tenant application",
    ),
    ("category.email", "Writing or answering email"),
    ("category.spreadsheet_fill", "Filling in a spreadsheet"),
    ("category.shop_order", "Ordering from a shop"),
    ("category.calendar_cleanup", "Tidying a calendar"),
    ("category.ride_order", "Ordering a ride"),
    ("category.daily_chart_check", "Checking a chart"),
    ("name.with_site", "{category} on {site}"),
    ("control.run_now", "Run now"),
    ("control.stop", "Stop"),
    ("control.switch_off", "Switch off"),
    ("control.switch_on", "Switch on"),
    ("control.delete", "Delete"),
    ("item.sign_in.title", "{name} needs you to sign in"),
    (
        "item.sign_in.text",
        "Sign in in its window, then choose Done.",
    ),
    ("item.link_account.title", "{name} needs your account"),
    (
        "item.link_account.text",
        "Link your mail or calendar account in its window, then choose Done.",
    ),
    ("item.stuck.title", "{name} is stuck"),
    (
        "item.stuck.text",
        "Do the next step in its window, then choose Done, or stop the task.",
    ),
    (
        "item.check.title",
        "Did the last task of {name} go through?",
    ),
    (
        "item.check.text",
        "Look at the details and tell it what happened.",
    ),
    ("item.setup.title", "Set up {name}"),
    ("item.setup.text", "Choose one answer for each question."),
    ("item.ready.title", "{name} is ready to start"),
    ("item.ready.text", "Start it now, or skip it this time."),
    ("item.show.title", "Show {name} the task"),
    (
        "item.show.text",
        "Do the task once in its window while it watches, then choose Done.",
    ),
    ("choice.sign_in.decline", "Not now"),
    ("choice.sign_in.done", "Done"),
    ("choice.link_account.decline", "Not now"),
    ("choice.link_account.done", "Done"),
    ("choice.stuck.decline", "Stop the task"),
    ("choice.stuck.done", "Done"),
    ("choice.check.try_again", "It did not go through"),
    ("choice.check.went_through", "It went through"),
    ("choice.setup.none", "Not now"),
    ("choice.setup.choose", "Use this"),
    ("choice.ready.skip", "Skip"),
    ("choice.ready.start", "Start"),
    ("choice.show.decline", "Not now"),
    ("choice.show.done", "Done"),
    ("param.start", "Where to start"),
    ("param.payee", "Who to pay"),
    ("param.instrument", "How to pay"),
    ("param.reply_senders", "Whose mail to answer"),
    ("param.workbook", "Which workbook"),
    ("param.sheet", "Which sheet"),
    ("param.range", "Which cells"),
    ("param.source", "Where the data comes from"),
    ("param.seller", "Which shop"),
    ("field.payment.payee", "To"),
    ("field.payment.amount", "Amount"),
    ("field.payment.instrument", "Paid with"),
    ("field.payment.origin", "Site"),
    ("field.purchase.seller", "Shop"),
    ("field.purchase.items", "Items"),
    ("field.purchase.amount", "Total"),
    ("field.purchase.instrument", "Paid with"),
    ("field.purchase.origin", "Site"),
    ("field.send.to", "To"),
    ("field.send.subject", "Subject"),
    ("field.send.body", "Message"),
    ("field.delete.items", "Items"),
    ("field.submit.origin", "Site"),
    ("field.submit.fields", "Details"),
    ("field.sign.document", "Document"),
    ("field.sign.party", "Signed with"),
    ("confirm.payment.title", "Pay this?"),
    ("confirm.payment.allow", "Pay"),
    ("confirm.payment.reject", "Don't pay"),
    ("confirm.payment.allow_many", "Pay all {count}"),
    ("confirm.payment.reject_many", "Don't pay these {count}"),
    ("confirm.purchase.title", "Place this order?"),
    ("confirm.purchase.allow", "Order"),
    ("confirm.purchase.reject", "Don't order"),
    ("confirm.purchase.allow_many", "Order all {count}"),
    ("confirm.purchase.reject_many", "Don't order these {count}"),
    ("confirm.sign.title", "Sign this?"),
    ("confirm.sign.allow", "Sign"),
    ("confirm.sign.reject", "Don't sign"),
    ("confirm.sign.allow_many", "Sign all {count}"),
    ("confirm.sign.reject_many", "Don't sign these {count}"),
    ("confirm.send.title", "Send this?"),
    ("confirm.send.allow", "Send"),
    ("confirm.send.reject", "Don't send"),
    ("confirm.send.allow_many", "Send all {count}"),
    ("confirm.send.reject_many", "Don't send these {count}"),
    ("confirm.delete.title", "Remove this?"),
    ("confirm.delete.allow", "Remove"),
    ("confirm.delete.reject", "Keep"),
    ("confirm.delete.allow_many", "Remove all {count}"),
    ("confirm.delete.reject_many", "Keep all {count}"),
    ("confirm.submit.title", "Submit this?"),
    ("confirm.submit.allow", "Submit"),
    ("confirm.submit.reject", "Don't submit"),
    ("confirm.submit.allow_many", "Submit all {count}"),
    ("confirm.submit.reject_many", "Don't submit these {count}"),
    ("confirm.new_site", "A site it has not used before"),
    ("confirm.present", "Open the window to answer"),
    ("from.you", "You gave this"),
    ("from.site", "Read on the site"),
    ("from.agent", "Written by the agent"),
    ("from.halvo", "Set by Halvo"),
    ("month.1", "January"),
    ("month.2", "February"),
    ("month.3", "March"),
    ("month.4", "April"),
    ("month.5", "May"),
    ("month.6", "June"),
    ("month.7", "July"),
    ("month.8", "August"),
    ("month.9", "September"),
    ("month.10", "October"),
    ("month.11", "November"),
    ("month.12", "December"),
    (
        "note.no_service",
        "Halvo cannot show your agents right now.",
    ),
    (
        "note.roster_full",
        "Halvo has as many agents as it can hold, so a new task gets none.",
    ),
    ("tray.needs", "{count} agents need you"),
];

fn sentence(key: &str) -> Option<&'static str> {
    KEYS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
}

/// The sentence of `key` with each placeholder filled by the value of its pair, in one pass, so
/// that a value is never read for placeholders; `malformed` for a key that the table does not
/// hold or a placeholder with no pair.
pub fn text(key: &str, pairs: &[(&str, &str)]) -> Result<String, Cause> {
    let mut rest = sentence(key).ok_or(Cause::Malformed)?;
    let mut out = String::with_capacity(rest.len());
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').ok_or(Cause::Malformed)? + open;
        let name = &rest[open + 1..close];
        let (_, value) = pairs
            .iter()
            .find(|(n, _)| *n == name)
            .ok_or(Cause::Malformed)?;
        out.push_str(value);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The output of `coord-words export`: `{"<key>":"<text>",...}` with the keys in byte order,
/// on one line with one LF at its end.
pub fn export() -> String {
    let map: BTreeMap<&str, &str> = KEYS.iter().copied().collect();
    let mut line = serde_json::to_string(&map).expect("a map of strings");
    line.push('\n');
    line
}

fn civil(day: u32) -> (i64, u32, u32) {
    let z = i64::from(day) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// A day, as days since 1970-01-01, written as a day number, a month name of the table and a
/// year: `3 November 2026` (I1.1).
pub fn day(day: u32) -> String {
    let (y, m, d) = civil(day);
    let month = sentence(&format!("month.{m}")).expect("the table holds the twelve months");
    format!("{d} {month} {y}")
}
