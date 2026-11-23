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
        "It could not get past one step and waits for your answer.",
    ),
    (
        "line.needs_you.check",
        "It needs you to say whether its last task went through.",
    ),
    (
        "line.needs_you.setup",
        "It needs a few answers from you before it can start.",
    ),
    (
        "line.needs_you.setup_owed",
        "It needs a few answers from you, but Halvo could not show the questions yet. Halvo tries again soon.",
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
        "Halvo switched it off after several tries in a row did not work. To try again, choose Switch on.",
    ),
    (
        "line.problem.record",
        "Halvo can't read or save this agent's record. The agent starts nothing new. Stop works if shown.",
    ),
    (
        "line.problem.quit",
        "Its task was still running when Halvo quit and may have gone on. Check outside Halvo before repeating it.",
    ),
    (
        "card.records_dropped",
        "After Halvo started, {count} of this agent's records went unsaved. Its steps may have happened; check outside Halvo before repeating.",
    ),
    (
        "card.history_busy",
        "Halvo could not show this agent's past tasks just now.",
    ),
    ("card.read_again", "Try again"),
    (
        "line.problem.sign_in_refused",
        "This agent needs you to sign in at the site.",
    ),
    (
        "line.problem.site_declined",
        "You did not allow a site, so this agent waits. Start it when it should ask again.",
    ),
    (
        "line.problem.no_browser",
        "This agent needs Google Chrome or Microsoft Edge to open web pages. Install one of them, then run the agent again.",
    ),
    (
        "line.problem.browser_start",
        "The browser did not start, so this agent could not open web pages. Run the agent again.",
    ),
    (
        "line.problem.no_channel",
        "The browser started but did not answer, so this agent could not open web pages. Run the agent again.",
    ),
    (
        "line.off.kept",
        "This task is switched off. To let it run again, choose Switch on.",
    ),
    (
        "line.all_set.nothing_first",
        "The first time it ran, it found nothing to do.",
    ),
    (
        "line.quitting",
        "Halvo is closing, so it starts nothing new.",
    ),
    (
        "line.run.answer_lost",
        "Halvo could not pass your answer on to the agent, so it stopped the task.",
    ),
    (
        "say_again",
        "Halvo could not use your answers for “{name}”, so it asks again.",
    ),
    ("line.working.starting", "It is starting."),
    ("line.working.working", "It is doing its task now."),
    ("line.working.stopping", "It is stopping."),
    (
        "line.working.kill_stuck_mac",
        "Its task may still run. Right-click Halvo in the Dock, choose Quit. Halvo stops all tasks; that can take seconds.",
    ),
    (
        "line.working.kill_stuck_windows",
        "Its task may still run. Click Halvo's icon by the clock (or under ^), then Quit Halvo.",
    ),
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
    ("line.all_set.new", "It has not done its task yet, or not for a long time."),
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
    ("control.take_back", "Cancel this start"),
    ("control.switch_off", "Switch off"),
    ("control.switch_on", "Switch on"),
    ("control.use_model", "Use an AI model"),
    ("control.this_computer_only", "Use this computer only"),
    ("control.stop_sign_in", "Stop using its sign-in"),
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
    ("item.stuck.title", "“{name}” could not get past one step"),
    (
        "item.stuck.text",
        "You can finish the task yourself in its own window, then choose Done, or choose Stop this time. Either way it tries again next time.",
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
    ("confirm.payment.reject_many", "Don't pay any of these {count}"),
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
    ("confirm.asked_by", "“{name}” asks for this."),
    (
        "confirm.asked_by_place",
        "Question {n} of {count}. The agent “{name}” asks for this.",
    ),
    ("confirm.field", "{label} ({source}): {value}"),
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
        "note.out_of_date",
        "This list may be out of date. To start an agent, close Halvo and open it again.",
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
    (
        "item.sign_in.site",
        "Sign in to {origin} in its own window, then choose Done.",
    ),
    (
        "item.sign_in.site_kept",
        "Sign in to {origin} in its own window, then choose Done. Halvo keeps this agent signed in to {origin} on this computer, so that it does not ask you each time. It removes the sign-in at the latest 35 days after this agent last finished a run, or when you delete everything.",
    ),
    (
        "item.sign_in.text_kept",
        "Sign in to the site in its own window, then choose Done. Halvo keeps this agent signed in to the site on this computer, so that it does not ask you each time. It removes the sign-in at the latest 35 days after this agent last finished a run, or when you delete everything.",
    ),
    (
        "choice.sign_in.bind_decline",
        "Don't allow this site, and end this task",
    ),
    (
        "choice.sign_in.bind_allow",
        "Allow this site now and whenever this agent runs again",
    ),
    (
        "item.setup.not_taken",
        "Halvo could not use those answers, so choose them again.",
    ),
    (
        "item.setup.overlap",
        "“{name}” already answers mail from {senders} on this mailbox, and a message that one agent answered can still look unread to the other. Take those people out of your list and choose again, or switch “{name}” off or delete it.",
    ),
    (
        "item.setup.overlap_more",
        "“{name}” already answers mail from {senders} on this mailbox, and other agents answer some of these people too. A message that one agent answered can still look unread to another. Take those people out of your list and choose again, or switch the other agents off or delete them.",
    ),
    (
        "item.setup.overlap_unread",
        "Another of your agents already answers mail from at least one of these people on this mailbox, and Halvo cannot show which one right now. A message that one agent answered can still look unread to the other. Take out the people that the other agent answers and choose again, or switch the other agent off or delete it.",
    ),
    ("run.effect.payment", "It made this payment."),
    ("run.effect.purchase", "It placed this order."),
    ("run.effect.send", "It sent this message."),
    ("run.effect.delete", "It removed this."),
    ("run.effect.submit", "It sent this form."),
    ("run.effect.sign", "It signed this document."),
    ("run.effect.local_edit", "It changed this file."),
    (
        "run.effect.payment.unshown",
        "It made a payment, but Halvo cannot show its details.",
    ),
    (
        "run.effect.purchase.unshown",
        "It placed an order, but Halvo cannot show its details.",
    ),
    (
        "run.effect.send.unshown",
        "It sent a message, but Halvo cannot show its details.",
    ),
    (
        "run.effect.delete.unshown",
        "It removed something, but Halvo cannot show its details.",
    ),
    (
        "run.effect.submit.unshown",
        "It sent a form, but Halvo cannot show its details.",
    ),
    (
        "run.effect.sign.unshown",
        "It signed a document, but Halvo cannot show its details.",
    ),
    (
        "run.effect.local_edit.unshown",
        "It changed a file, but Halvo cannot show its details.",
    ),
    (
        "run.effect.unshown",
        "It did one thing that Halvo cannot show.",
    ),
    (
        "run.effect.payment.partial",
        "It made this payment, but Halvo shows only the details it could read.",
    ),
    (
        "run.effect.purchase.partial",
        "It placed this order, but Halvo shows only the details it could read.",
    ),
    (
        "run.effect.send.partial",
        "It sent this message, but Halvo shows only the details it could read.",
    ),
    (
        "run.effect.submit.partial",
        "It sent this form, but Halvo shows only the details it could read.",
    ),
    (
        "run.effect.sign.partial",
        "It signed this document, but Halvo shows only the details it could read.",
    ),
    (
        "run.effect.local_edit.partial",
        "It changed a file, but Halvo shows only the details it could read.",
    ),
    ("run.effect.payment.not_done", "This payment was not made."),
    ("run.effect.purchase.not_done", "This order was not placed."),
    ("run.effect.send.not_done", "This message was not sent."),
    (
        "run.effect.delete.not_done",
        "Something it was going to remove was not removed.",
    ),
    ("run.effect.submit.not_done", "This form was not sent."),
    ("run.effect.sign.not_done", "This document was not signed."),
    ("run.effect.local_edit.not_done", "This file was not changed."),
    ("run.effect.not_done", "One step of this task was not done."),
    (
        "run.effect.payment.refused",
        "You said no, so this payment was not made.",
    ),
    (
        "run.effect.purchase.refused",
        "You said no, so this order was not placed.",
    ),
    (
        "run.effect.send.refused",
        "You said no, so this message was not sent.",
    ),
    (
        "run.effect.delete.refused",
        "You said no, so something it was going to remove was not removed.",
    ),
    (
        "run.effect.submit.refused",
        "You said no, so this form was not sent.",
    ),
    (
        "run.effect.sign.refused",
        "You said no, so this document was not signed.",
    ),
    (
        "run.effect.local_edit.refused",
        "You said no, so this file was not changed.",
    ),
    (
        "run.effect.refused",
        "You said no, so one step of this task was not done.",
    ),
    (
        "run.effect.payment.unknown",
        "A payment may have been made; Halvo could not tell. Check your bank or the site before you pay again.",
    ),
    (
        "run.effect.purchase.unknown",
        "An order may have been placed; Halvo could not tell. Check the shop's order page before you order again.",
    ),
    (
        "run.effect.send.unknown",
        "A message may have been sent; Halvo could not tell. Look in your sent messages before you send it again.",
    ),
    (
        "run.effect.delete.unknown",
        "Something may have been removed; Halvo could not tell. Look where this agent works for anything missing.",
    ),
    (
        "run.effect.submit.unknown",
        "A form may have been sent; Halvo could not tell. Check the site before you send it again.",
    ),
    (
        "run.effect.sign.unknown",
        "A document may have been signed for you; Halvo could not tell. Check your email before you sign it again.",
    ),
    (
        "run.effect.local_edit.unknown",
        "A file may have been changed; Halvo could not tell. Open it and look before you change it again.",
    ),
    (
        "run.effect.unknown",
        "One step may have been done; Halvo could not tell. Check where this agent works before you run it again.",
    ),
    ("line.run.stopped", "It stopped before the task was done."),
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
    (
        "line.working.deleting",
        "Halvo is stopping this agent and will then remove it.",
    ),
    ("ask.delete.title", "Remove “{name}” from Halvo?"),
    ("ask.delete.next", "Its next task is set for {day}."),
    (
        "ask.delete.setup",
        "Halvo removes what you set up and answered for it, and what it learned.",
    ),
    (
        "ask.delete.sign_in",
        "It also removes its schedule and what it kept on this computer to sign in.",
    ),
    (
        "ask.delete.records",
        "It removes the record of each task and of what it did.",
    ),
    ("ask.delete.final", "You cannot bring it back."),
    (
        "ask.delete.done",
        "Nothing it already paid, ordered or sent is taken back, and nothing at a website is cancelled.",
    ),
    (
        "ask.delete.copies",
        "The copies that Halvo made beside your workbooks stay.",
    ),
    (
        "ask.delete.starts",
        "If it starts a task before you answer, Halvo stops it first and removes it once it has stopped.",
    ),
    (
        "ask.delete.record",
        "Halvo keeps no record of this once the agent is removed.",
    ),
    ("ask.delete.keep", "Keep it"),
    (
        "ask.stop_sign_in.title",
        "Stop using the sign-in of “{name}”?",
    ),
    (
        "ask.stop_sign_in.stops",
        "Halvo will stop using the sign-in that it keeps for this agent and remove it from this computer.",
    ),
    (
        "ask.stop_sign_in.next",
        "The next time the agent runs, it asks you to sign in again.",
    ),
    ("ask.stop_sign_in.keep", "Keep the sign-in"),
    (
        "ask.effect.payment",
        "Halvo does not know whether it made this payment.",
    ),
    (
        "ask.effect.purchase",
        "Halvo does not know whether it placed this order.",
    ),
    (
        "ask.effect.send",
        "Halvo does not know whether it sent this message.",
    ),
    (
        "ask.effect.delete",
        "Halvo does not know whether it removed this.",
    ),
    (
        "ask.effect.submit",
        "Halvo does not know whether it sent this form.",
    ),
    (
        "ask.effect.sign",
        "Halvo does not know whether it signed this document.",
    ),
    (
        "ask.effect.local_edit",
        "Halvo does not know whether it changed this file.",
    ),
    (
        "ask.effect.other",
        "Halvo does not know whether one step of its last task happened.",
    ),
    ("ask.working.title", "“{name}” is working now"),
    (
        "ask.working.text",
        "Stop it first, and you can remove it once it has stopped.",
    ),
    ("ask.working.keep", "Let it work"),
    ("ask.stopping.title", "“{name}” is stopping"),
    (
        "ask.stopping.text",
        "You can remove it once it has stopped.",
    ),
    ("ask.stopping.done", "OK"),
    (
        "line.needs_you.asks_again",
        "You cancelled its start, so it asks you again before it starts.",
    ),
    (
        "refused.stale",
        "This question is no longer open, so Halvo did nothing with your choice.",
    ),
    (
        "refused.presenter",
        "This question cannot be answered here, so Halvo did nothing.",
    ),
    (
        "refused.early",
        "Halvo was not ready for that yet, so it did nothing. Try again.",
    ),
    (
        "refused.hold",
        "Halvo is holding it back for now, so it did not start.",
    ),
    (
        "refused.state",
        "That does not fit what it is doing now, so nothing changed.",
    ),
    (
        "refused.budget",
        "It has started as often as it may today, so it did not start again.",
    ),
    (
        "refused.queue_full",
        "Too many agents are waiting their turn, so it did not start. Try again later.",
    ),
    (
        "refused.busy",
        "Halvo is busy for a moment, so nothing happened. Try again.",
    ),
    (
        "refused.no_service",
        "Halvo cannot do that right now, so nothing happened. Try again later.",
    ),
    (
        "refused.unknown_item",
        "Halvo no longer has this question, so it did nothing with your choice.",
    ),
    (
        "refused.unknown_agent",
        "This agent is no longer in your list, so nothing happened.",
    ),
    (
        "refused.malformed",
        "Something went wrong inside Halvo, so nothing happened. Try again.",
    ),
    (
        "refused.overlap",
        "Another of your agents already answers mail from some of the same people on this mailbox, and Halvo cannot show which one right now. A message that one agent answered can still look unread to the other, so this agent stays off. Switch the other agent off or delete it, then switch this one on.",
    ),
    (
        "refused.overlap_named",
        "“{name}” already answers mail from {senders} on this mailbox, and a message that one agent answered can still look unread to the other, so this agent stays off. Switch “{name}” off or delete it, then switch this one on.",
    ),
    (
        "refused.overlap_more",
        "“{name}” already answers mail from {senders} on this mailbox, and other agents answer some of these people too. A message that one agent answered can still look unread to another, so this agent stays off. Switch the other agents off or delete them, then switch this one on.",
    ),
    (
        "refused.day",
        "Halvo cannot do that today because this computer's clock was changed. Try again in a day or two.",
    ),
    (
        "refused.switch_not_saved",
        "Halvo is stopping the task but could not switch this agent off. It may start again: press Switch off again.",
    ),
    (
        "refused.sign_in_not_finished",
        "Halvo could not finish this. If this agent still shows Stop using its sign-in, choose it again.",
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
