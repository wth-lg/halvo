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
    ("say.needs_you", "“{name}” needs you."),
    ("say.problem", "“{name}” has a problem."),
    ("say.working", "“{name}” is working."),
    ("say.waiting_turn", "“{name}” is waiting its turn."),
    ("say.all_set", "“{name}” is all set."),
    ("say.off", "“{name}” is switched off."),
    ("say.not_yet", "Halvo cannot do “{name}” yet."),
    (
        "line.needs_you.confirm",
        "It needs your answer before it can go on.",
    ),
    (
        "line.needs_you.sign_in",
        "It needs you to sign in to a site.",
    ),
    (
        "line.needs_you.link_account",
        "It needs you to link your mail or calendar account.",
    ),
    (
        "line.needs_you.stuck",
        "It is stuck and needs your help with one step.",
    ),
    (
        "line.needs_you.check",
        "It needs you to say whether its last task went through.",
    ),
    (
        "line.needs_you.setup",
        "It needs a few answers from you before it can start.",
    ),
    ("line.needs_you.ready", "It is ready to start when you are."),
    (
        "line.needs_you.show",
        "It needs you to show it how to do the task once.",
    ),
    (
        "line.not_yet.category",
        "Halvo cannot do this kind of task yet.",
    ),
    (
        "line.off.person",
        "You switched it off, so it does not start.",
    ),
    (
        "line.problem.failures",
        "It failed several times in a row, so Halvo switched it off.",
    ),
    ("line.working.starting", "It is starting."),
    ("line.working.working", "It is doing its task now."),
    ("line.working.stopping", "It is stopping."),
    (
        "line.waiting_turn.queued",
        "Only one agent works at a time, so it starts when its turn comes.",
    ),
    (
        "line.problem.failed",
        "Last time, it could not finish its task.",
    ),
    (
        "line.problem.limit",
        "Last time, it took too long, so Halvo stopped it.",
    ),
    (
        "line.problem.class_mismatch",
        "Last time, Halvo stopped it before it did something that it is not allowed to do.",
    ),
    (
        "line.problem.refused",
        "Last time, Halvo stopped one of its steps, so the task was not done.",
    ),
    (
        "line.problem.budget",
        "Last time, it asked you too many questions, so Halvo stopped it.",
    ),
    (
        "line.problem.kind",
        "Last time, it asked for something that Halvo does not allow, so Halvo stopped it.",
    ),
    (
        "line.all_set.tried_today",
        "It has started as often as it may today, so it waits until tomorrow.",
    ),
    ("line.all_set.new", "It has not done its task yet."),
    ("line.all_set.done", "Last time, it finished its task."),
    ("line.all_set.nothing", "Last time, it found nothing to do."),
    (
        "line.all_set.stopped_early",
        "Last time, it stopped before the task was done, and it needs nothing from you now.",
    ),
    ("line.run.completed", "It finished the task."),
    ("line.run.nothing_to_do", "It found nothing to do."),
    ("line.run.declined", "You said no, so it stopped."),
    (
        "line.run.handed_back",
        "It stopped at a step that it could not get past, and it tries again next time.",
    ),
    (
        "line.run.class_mismatch",
        "It stopped before it did something that it is not allowed to do.",
    ),
    (
        "line.run.refused",
        "Halvo stopped one of its steps, so the task was not done.",
    ),
    ("line.run.limit", "It took too long, so Halvo stopped it."),
    (
        "line.run.effect_unknown",
        "It could not tell whether its last step went through.",
    ),
    ("line.run.failed", "It could not finish the task."),
    ("line.run.cancelled", "You stopped it."),
    (
        "line.run.lapsed",
        "It stopped because you did not answer in time.",
    ),
    ("line.run.interrupted", "It stopped because Halvo closed."),
    (
        "line.run.deleted",
        "It stopped because the agent was deleted.",
    ),
    (
        "line.run.budget",
        "It asked you too many questions, so Halvo stopped it.",
    ),
    (
        "line.run.kind",
        "It asked for something that Halvo does not allow, so Halvo stopped it.",
    ),
    ("line.level.low", "Quick"),
    ("line.level.medium", "Standard"),
    ("line.level.high", "Thorough"),
    ("line.runs_on.on_device", "It runs on this computer."),
    ("line.asks.observe", "It only looks and never acts for you."),
    (
        "line.asks.draft_and_confirm",
        "It asks you before it pays, orders, sends, signs, submits, or removes anything.",
    ),
    (
        "line.asks.unattended",
        "It asks before anything you did not allow, and always before paying, ordering or signing.",
    ),
    (
        "line.time",
        "It stops if its task takes more than {minutes} minutes.",
    ),
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
    ("control.run_now", "Start now"),
    ("control.stop", "Stop"),
    ("control.switch_off", "Switch off"),
    ("control.switch_on", "Switch on"),
    ("control.delete", "Delete"),
    ("item.sign_in.title", "Sign in for “{name}”"),
    (
        "item.sign_in.text",
        "Sign in to the site in its own window, then choose Done.",
    ),
    ("item.link_account.title", "Link your account for “{name}”"),
    (
        "item.link_account.text",
        "Link your mail or calendar account in its own window, then choose Done.",
    ),
    ("item.stuck.title", "Help “{name}” with one step"),
    (
        "item.stuck.text",
        "Do the next step in its own window and choose Done, or choose Stop this time.",
    ),
    (
        "item.check.title",
        "Did the last task of “{name}” go through?",
    ),
    (
        "item.check.text",
        "Check the site or your account to see if this happened; it asks again before it pays.",
    ),
    ("item.setup.title", "Answer a few questions for “{name}”"),
    (
        "item.setup.text",
        "Choose one answer for each question, then choose Use these answers.",
    ),
    ("item.ready.title", "“{name}” is ready to start"),
    ("item.ready.text", "Start it now, or skip it this time."),
    ("item.show.title", "Show “{name}” how to do the task"),
    (
        "item.show.text",
        "Do the task once in its own window while it watches, then choose Done.",
    ),
    ("choice.sign_in.decline", "Stop this time"),
    ("choice.sign_in.done", "Done"),
    ("choice.link_account.decline", "Stop this time"),
    ("choice.link_account.done", "Done"),
    ("choice.stuck.decline", "Stop this time"),
    ("choice.stuck.done", "Done"),
    ("choice.check.try_again", "No, try again next time"),
    ("choice.check.went_through", "Yes, it went through"),
    ("choice.setup.none", "Not now"),
    ("choice.setup.choose", "Use these answers"),
    ("choice.ready.skip", "Skip this time"),
    ("choice.ready.start", "Start now"),
    ("choice.show.decline", "Stop this time"),
    ("choice.show.done", "Done"),
    ("param.start", "Where to start"),
    ("param.payee", "Who to pay"),
    ("param.instrument", "How to pay"),
    ("param.reply_senders", "Whose mail to answer"),
    ("param.workbook", "Which spreadsheet file"),
    ("param.sheet", "Which sheet"),
    ("param.range", "Which cells"),
    ("param.source", "Where the data comes from"),
    ("param.seller", "Which shop"),
    ("field.payment.payee", "Pay to"),
    ("field.payment.amount", "Amount"),
    ("field.payment.instrument", "Pay with"),
    ("field.payment.origin", "Site"),
    ("field.purchase.seller", "Shop"),
    ("field.purchase.items", "Items"),
    ("field.purchase.amount", "Total"),
    ("field.purchase.instrument", "Pay with"),
    ("field.purchase.origin", "Site"),
    ("field.send.to", "To"),
    ("field.send.subject", "Subject"),
    ("field.send.body", "Message"),
    ("field.delete.items", "What to remove"),
    ("field.submit.origin", "Site"),
    ("field.submit.fields", "What it fills in"),
    ("field.sign.document", "Document"),
    ("field.sign.party", "Who else signs"),
    ("confirm.payment.title", "Pay this?"),
    ("confirm.payment.allow", "Pay"),
    ("confirm.payment.reject", "Don't pay"),
    ("confirm.payment.allow_many", "Pay all {count}"),
    ("confirm.payment.reject_many", "Don't pay these {count}"),
    ("confirm.payment.action", "It will make this payment"),
    ("confirm.purchase.title", "Place this order?"),
    ("confirm.purchase.allow", "Order"),
    ("confirm.purchase.reject", "Don't order"),
    ("confirm.purchase.allow_many", "Order all {count}"),
    ("confirm.purchase.reject_many", "Don't order these {count}"),
    ("confirm.purchase.action", "It will place this order"),
    ("confirm.sign.title", "Sign this?"),
    ("confirm.sign.allow", "Sign"),
    ("confirm.sign.reject", "Don't sign"),
    ("confirm.sign.allow_many", "Sign all {count}"),
    ("confirm.sign.reject_many", "Don't sign these {count}"),
    ("confirm.sign.action", "It will sign this document"),
    ("confirm.send.title", "Send this?"),
    ("confirm.send.allow", "Send"),
    ("confirm.send.reject", "Don't send"),
    ("confirm.send.allow_many", "Send all {count}"),
    ("confirm.send.reject_many", "Don't send these {count}"),
    ("confirm.send.action", "It will send this message"),
    ("confirm.delete.title", "Remove this?"),
    ("confirm.delete.allow", "Remove"),
    ("confirm.delete.reject", "Keep"),
    ("confirm.delete.allow_many", "Remove all {count}"),
    ("confirm.delete.reject_many", "Keep all {count}"),
    ("confirm.delete.action", "It will remove this"),
    ("confirm.submit.title", "Submit this?"),
    ("confirm.submit.allow", "Submit"),
    ("confirm.submit.reject", "Don't submit"),
    ("confirm.submit.allow_many", "Submit all {count}"),
    ("confirm.submit.reject_many", "Don't submit these {count}"),
    ("confirm.submit.action", "It will submit this form"),
    ("confirm.mixed.title", "Do these {count} things?"),
    ("confirm.mixed.reject", "Don't do any of these {count}"),
    ("confirm.mixed.allow", "Do all {count}"),
    ("confirm.new_site", "A site it has not used before"),
    (
        "confirm.changed",
        "Something changed after you said yes, so it has not done this yet and asks you again.",
    ),
    ("confirm.present", "See the details and answer"),
    ("from.you", "You gave this"),
    ("from.site", "From the site"),
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
        "You have as many agents as Halvo can hold, so it cannot add one for a new task.",
    ),
    ("tray.needs", "Agents that need you: {count}"),
    ("item.link_account.origin", "Site of your account"),
    (
        "item.sign_in.bind",
        "This agent will now work on {origin}. Allow this site only if you know it.",
    ),
    ("choice.sign_in.bind_decline", "Don't allow this site"),
    ("choice.sign_in.bind_allow", "Allow this site"),
    (
        "item.setup.not_taken",
        "Halvo could not use those answers, so choose them again.",
    ),
    ("run.effect.payment", "It made this payment."),
    ("run.effect.purchase", "It placed this order."),
    ("run.effect.send", "It sent this message."),
    ("run.effect.delete", "It removed this."),
    ("run.effect.submit", "It sent this form."),
    ("run.effect.sign", "It signed this document."),
    ("run.effect.local_edit", "It changed this file."),
    ("field.local_edit.file", "File"),
    ("field.local_edit.range", "Where in the file"),
    (
        "line.waiting_turn.store",
        "It waits while Halvo wraps up the task it did before, and you need to do nothing.",
    ),
    ("say.item.new", "“{name}” has a new question for you."),
    (
        "say.item.gone",
        "“{name}” no longer has a question for you.",
    ),
    (
        "param_typed.full",
        "Your answer is full, so the last thing you added was left out. Shorten it to add more.",
    ),
    (
        "param_typed.too_long",
        "Something you added was too long to fit, so none of it was added. Read your answer before sending.",
    ),
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
