use super::*;
use crate::mem::Config;
use crate::types::fixtures::{facts, State};
use crate::types::{ModelUse, SpendLimit};

const NAME: &str = "Pay bills on Site1";

/// A configuration of `tier` and `runtime` with `model` as the app hands it.
fn config(tier: Tier, runtime: Runtime, model: Option<ModelUse>) -> Config {
    let mut config = Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{:032x}\",\"agent\":\"{:032x}\",\"rev\":1,\"category\":\"utility_bill_payment\",\"tier\":\"{}\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"yes\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"{}\",\"playbook\":null}}",
        1,
        2,
        tier.name(),
        runtime.name()
    ))
    .unwrap();
    config.set_model(model);
    config
}

/// What the app hands of a host with one model, a saved key and a limit, with `stands`.
fn model(stands: bool) -> ModelUse {
    ModelUse {
        host: "hosted-model.test".to_owned(),
        models: "test-model-small".to_owned(),
        stands,
        key: Some(true),
        limit: SpendLimit::Stands {
            amount: "20.00 USD".to_owned(),
            period: "month".to_owned(),
            near: false,
        },
        hands_back: false,
    }
}

/// What the app hands of a host with one model, with `stands`, the key's state `key` and the limit
/// `limit` (r3 U7.8).
fn model_with(stands: bool, key: Option<bool>, limit: SpendLimit) -> ModelUse {
    ModelUse {
        key,
        limit,
        ..model(stands)
    }
}

/// A limit of 20.00 USD a month that reads near or not (r3 U7.8).
fn limit(near: bool) -> SpendLimit {
    SpendLimit::Stands {
        amount: "20.00 USD".to_owned(),
        period: "month".to_owned(),
        near,
    }
}

/// A web agent at `tier` with no playbook, whose one run, started by the person today, ended
/// `handed_back` with the last cause `cause`, and beside whose configuration the app hands `model`
/// (r3 U7.8).
fn u78_handed_back(tier: Tier, cause: Option<LastCause>, model: Option<ModelUse>) -> Facts<Config> {
    let mut f = facts(State::Ten).remove(0);
    f.config = config(tier, Runtime::Web, model);
    f.events.clear();
    f.runs.clear();
    let run = RunId::parse(&format!("{:032x}", 0x7880)).unwrap();
    let whats = [
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run,
            cause: RunEnd::HandedBack,
            defers: None,
        },
    ];
    for what in whats {
        f.events.push(Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.clone(),
            seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
            day: f.today,
            at: i64::from(f.today) * 86_400_000_000_000,
            what,
        });
    }
    f.last_cause = cause;
    f
}

/// r3 U7.8: an agent whose newest run handed its work back for a cause of a hosted model rests
/// while that cause stands by what the app hands at the tick: the tick raises no `ready` and starts
/// nothing, and the person's own start passes; once the facts show the cause gone, the tick raises
/// `ready` again. Where the app hands no use of a model, where the key's state does not read for
/// `no_model_key`, or where the limit does not read for `over_cap`, it rests.
#[test]
fn u78_an_agent_that_handed_back_for_a_cause_of_a_hosted_model_asks_again_only_once_the_cause_is_gone(
) {
    use crate::schedule::{start_step, tick_step, Around, Verdict};
    let ok = model_with(true, Some(true), limit(false));
    let rows = [
        (
            "no_agreement stands",
            Tier::Frontier,
            LastCause::NoAgreement,
            Some(model(false)),
            true,
        ),
        (
            "no_agreement, no use of a model handed",
            Tier::Frontier,
            LastCause::NoAgreement,
            None,
            true,
        ),
        (
            "no_agreement gone: the yes stands",
            Tier::Frontier,
            LastCause::NoAgreement,
            Some(ok.clone()),
            false,
        ),
        (
            "no_model_key stands",
            Tier::Frontier,
            LastCause::NoModelKey,
            Some(model_with(true, Some(false), limit(false))),
            true,
        ),
        (
            "no_model_key, the key's state does not read",
            Tier::Frontier,
            LastCause::NoModelKey,
            Some(model_with(true, None, limit(false))),
            true,
        ),
        (
            "no_model_key gone: a key is saved",
            Tier::Frontier,
            LastCause::NoModelKey,
            Some(ok.clone()),
            false,
        ),
        (
            "over_cap stands: the limit reads near",
            Tier::Frontier,
            LastCause::OverCap,
            Some(model_with(true, Some(true), limit(true))),
            true,
        ),
        (
            "over_cap, the limit does not read",
            Tier::Frontier,
            LastCause::OverCap,
            Some(model_with(true, Some(true), SpendLimit::Unread)),
            true,
        ),
        (
            "over_cap gone: the limit does not read near",
            Tier::Frontier,
            LastCause::OverCap,
            Some(ok.clone()),
            false,
        ),
        (
            "over_cap gone: no limit is set",
            Tier::Frontier,
            LastCause::OverCap,
            Some(model_with(true, Some(true), SpendLimit::NotSet)),
            false,
        ),
        (
            "shortfall stands",
            Tier::Frontier,
            LastCause::Shortfall,
            Some(ok.clone()),
            true,
        ),
        (
            "shortfall gone at the way back to on_device, where the agent rests as one that was never allowed a model (r3 U7.11)",
            Tier::OnDevice,
            LastCause::Shortfall,
            Some(model(false)),
            true,
        ),
    ];
    let around = Around {
        listed: true,
        live: 0,
        model_held: false,
        queued: 0,
        window: false,
        from_queue: false,
        owed: false,
    };
    for (row, tier, cause, model, rests) in rows {
        let f = u78_handed_back(tier, Some(cause), model);
        let ask = tick_step(&f, true, &Limits::PRODUCT);
        let start = start_step(&f, Trigger::Person, &Limits::PRODUCT, &around);
        println!("row: {row}: tick {ask:?}, a start of the person {start:?}");
        let want = if rests {
            crate::schedule::Ask::Nothing
        } else {
            crate::schedule::Ask::Ready
        };
        assert_eq!(ask, want, "{row}");
        assert!(matches!(start.verdict, Verdict::Go(_)), "{row}: {start:?}");
    }
}

/// r4 U3.17b sentence 1, as the coordinator's ruling of log line 18496 turns it: a row offers Use an AI model where
/// the app hands the agent's model (this build's table of hosts holds a row and names one origin), the agent's runtime
/// asks a model (`MODEL_RUNTIMES`), and its stored tier is `on_device` or its stored yes does not stand; it offers Use
/// this computer only where its stored tier is a hosted tier; both stand before Delete. Where the app hands none, as
/// with a table of hosts that holds no row, a row offers neither, whatever its tier; a file agent in a build with a
/// host is offered nothing.
#[test]
fn a_row_offers_the_two_controls_of_an_ai_model_by_the_tier_the_yes_and_the_runtime() {
    use Control::{ThisComputerOnly as Back, UseModel as Use};
    let rows = [
        (Tier::OnDevice, Runtime::Web, None, vec![]),
        (Tier::Frontier, Runtime::Web, None, vec![]),
        (Tier::OnDevice, Runtime::Web, Some(model(false)), vec![Use]),
        (Tier::OnDevice, Runtime::None, Some(model(false)), vec![]),
        (Tier::OnDevice, Runtime::File, Some(model(false)), vec![]),
        (
            Tier::LowCostHosted,
            Runtime::Web,
            Some(model(true)),
            vec![Back],
        ),
        (Tier::Frontier, Runtime::Web, Some(model(true)), vec![Back]),
        (
            Tier::Frontier,
            Runtime::Web,
            Some(model(false)),
            vec![Use, Back],
        ),
    ];
    let base = facts(State::Ten).remove(0);
    for (tier, runtime, model, added) in rows {
        let mut f = base.clone();
        f.config = config(tier, runtime, model.clone());
        let line = status(&f, &Limits::PRODUCT, None);
        let without = {
            let mut g = f.clone();
            g.config.set_model(None);
            row(&g, line, NAME, None, &Limits::PRODUCT).unwrap()
        };
        let mut want: Vec<Control> = without.controls.iter().map(|c| c.control).collect();
        assert_eq!(want.last(), Some(&Control::Delete), "{want:?}");
        let at = want.len() - 1;
        want.splice(at..at, added.iter().copied());
        let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
        let shown: Vec<(Control, &str)> = r
            .controls
            .iter()
            .map(|c| (c.control, c.text.as_str()))
            .collect();
        assert_eq!(
            shown.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            want,
            "{tier} {runtime} {model:?}"
        );
        for (control, text) in shown {
            match control {
                Use => assert_eq!(text, "Use an AI model"),
                Back => assert_eq!(text, "Use this computer only"),
                _ => {}
            }
        }
    }
}

/// r4 U3.17b sentence 6: the line of the card on where an agent runs, one card for each case:
/// It runs on this computer. where the stored tier is `on_device`; It may use an AI model at
/// {host}: {models}. where it is a hosted tier and the stored yes stands for it; and The AI model
/// for this agent is now at {host}. Allow it again? where it is a hosted tier and the yes does not
/// stand, each byte for byte.
#[test]
fn the_cards_line_says_where_the_agent_runs_by_its_tier_and_its_yes() {
    let two = ModelUse {
        models: "test-model-small and test-model-large".to_owned(),
        ..model(true)
    };
    let rows = [
        (Tier::OnDevice, model(false), "It runs on this computer."),
        (
            Tier::Frontier,
            two.clone(),
            "It may use an AI model at hosted-model.test: test-model-small and test-model-large.",
        ),
        (
            Tier::LowCostHosted,
            ModelUse {
                stands: false,
                ..two
            },
            "The AI model for this agent is now at hosted-model.test. Allow it again?",
        ),
    ];
    let base = facts(State::Ten).remove(0);
    for (tier, model, want) in rows {
        let mut f = base.clone();
        f.config = config(tier, Runtime::Web, Some(model));
        let card = card(&f, NAME, &[], &Limits::PRODUCT).unwrap();
        let runs_on: Vec<&str> = card
            .lines
            .iter()
            .filter(|line| line.key == LineKey::RunsOn)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(runs_on, [want], "{tier}");
    }
}

/// r4 U3.22a sentence 5: the row of an agent of the runtime file whose newest run ended with a
/// cause of the failure class says why by the roster's last cause (r4 U3.19), for each of the
/// thirteen causes of a failed file run, byte for byte: the four that the record holds today and
/// the nine finer ones of the agents run's unit r3 U11.3 (the coordinator's log line 18826); an
/// agent of the runtime web with one of these thirteen names reads today's line of a failed run.
#[test]
fn u322a_a_failed_file_run_says_why_by_its_last_cause_and_a_web_run_reads_as_today() {
    use crate::types::{Failure, LastCause};
    let today = "Last time, it could not finish its task.";
    let rows = [
        (
            "workbook",
            "Halvo cannot change this file. It must be an Excel workbook (.xlsx) with no macros, pivot tables, data validation or password.",
        ),
        (
            "sheet",
            "Halvo could not check its own changes, so it left your spreadsheet file as it was.",
        ),
        (
            "changed",
            "The spreadsheet file changed while the agent worked, so Halvo left it as it was. Choose Start now to try again.",
        ),
        (
            "step_failed",
            "It could not fill in the spreadsheet. Check the file, the sheet, the cells and the data file that you gave it. Then choose Change its answers.",
        ),
        (
            "no_workbook",
            "Halvo did not find the spreadsheet file where you said it is. Then choose Change its answers.",
        ),
        (
            "no_sheet",
            "The spreadsheet file has no sheet with the name you gave. Then choose Change its answers.",
        ),
        (
            "no_range",
            "Halvo cannot read the cells you gave. They must look like A2:C11. Then choose Change its answers.",
        ),
        (
            "data_shape",
            "The data does not have exactly as many rows and columns as the cells you gave. Then choose Change its answers.",
        ),
        (
            "no_data_file",
            "Halvo did not find the data file where you said it is. Then choose Change its answers.",
        ),
        (
            "data_outside",
            "The data file is not in the same folder as the spreadsheet file. Then choose Change its answers.",
        ),
        (
            "data_form",
            "Halvo cannot read the data file. Each line must be one row with commas, semicolons or tabs between its values, and every row as long as the first. Then choose Change its answers.",
        ),
        (
            "data_encoding",
            "Halvo cannot read the letters in the data file. Open the file in your spreadsheet program and save it again under the same name as a CSV file with UTF-8 text; Excel calls this file type CSV UTF-8. Then choose Start now.",
        ),
        (
            "two_sheets",
            "The spreadsheet file has two sheets with the name you gave, so Halvo did not know which one to fill. Open the file in your spreadsheet program, give each sheet a name of its own and save it. Then choose Change its answers.",
        ),
        (
            "data_narrow",
            "A value in the data file is only number signs (###). A spreadsheet program writes that when a column is too narrow to show a value. In the spreadsheet you saved the file from, widen that column and save the data file again under the same name. Then choose Start now.",
        ),
        (
            "not_replaced",
            "Halvo could not save the spreadsheet file. Close it in other programs and check that you may change it, then choose Start now.",
        ),
        (
            "read_only",
            "Halvo may not change the spreadsheet file: it is read-only.",
        ),
    ];
    let base = facts(State::Ten).remove(0);
    let run = RunId::parse(&format!("{:032x}", 0x7701)).unwrap();
    for (name, said) in rows {
        for (runtime, want) in [(Runtime::File, said), (Runtime::Web, today)] {
            let mut f = base.clone();
            f.config = config(Tier::OnDevice, runtime, None);
            for what in [
                What::RunStarted {
                    run: run.clone(),
                    trigger: Trigger::Person,
                    slot: None,
                },
                What::RunEnded {
                    run: run.clone(),
                    cause: RunEnd::from(Failure::Failed),
                    defers: None,
                },
            ] {
                f.events.push(Event {
                    workflow: f.workflow.id.clone(),
                    agent: f.agent.clone(),
                    seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
                    day: f.today,
                    at: i64::from(f.today) * 86_400_000_000_000,
                    what,
                });
            }
            f.last_cause = LastCause::parse(name).ok();
            let line = status(&f, &Limits::PRODUCT, None);
            let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
            assert_eq!(r.line, want, "{name} {runtime}");
        }
    }
}

/// r4 U3.22d: the row of an agent of the runtime file whose newest run ended with the cause
/// `no_access` of the agents run's unit r3 U11.4 says the sentence of the build's system, byte for
/// byte (the coordinator's log lines 19024 and 19036), and the row of this build says this
/// build's; an agent of the runtime web with that cause reads today's line of a failed run.
#[test]
fn u322d_a_file_run_that_the_system_refused_says_what_to_do_on_each_system() {
    use crate::types::{Failure, LastCause};
    let today = "Last time, it could not finish its task.";
    let mac = "macOS did not let Halvo work with the spreadsheet file or the data file. Open System Settings > Privacy & Security > Files & Folders and under Halvo turn on the folder that holds them. If Halvo is not listed there, or the folder is already on, check that you may open and change both files. Then quit Halvo, open it again and choose Start now.";
    let windows = "Windows did not let Halvo work with the spreadsheet file or the data file. Check that you may open both files and change files in their folder, and that Windows Security does not block Halvo. Then choose Start now.";
    let base = facts(State::Ten).remove(0);
    let run = RunId::parse(&format!("{:032x}", 0x7702)).unwrap();
    for (runtime, on_mac, on_windows) in
        [(Runtime::File, mac, windows), (Runtime::Web, today, today)]
    {
        let mut f = base.clone();
        f.config = config(Tier::OnDevice, runtime, None);
        for what in [
            What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
            What::RunEnded {
                run: run.clone(),
                cause: RunEnd::from(Failure::Failed),
                defers: None,
            },
        ] {
            f.events.push(Event {
                workflow: f.workflow.id.clone(),
                agent: f.agent.clone(),
                seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
                day: f.today,
                at: i64::from(f.today) * 86_400_000_000_000,
                what,
            });
        }
        f.last_cause = LastCause::parse("no_access").ok();
        let line = status(&f, &Limits::PRODUCT, None);
        for (system, want) in [(System::Mac, on_mac), (System::Windows, on_windows)] {
            let r = row_on(system, &f, false, line, NAME, None, &Limits::PRODUCT).unwrap();
            assert_eq!(r.line, want, "{system:?} {runtime}");
            if system == System::HOST {
                let here = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
                assert_eq!(here.line, want, "this build, {runtime}");
            }
        }
    }
}

/// r3 U7.2a, as r3 U7.8 turns it: the row of an agent whose newest run ended `handed_back` with one
/// of the causes of a hosted model says the ruled problem line of that cause, byte for byte, while
/// the agent rests (`crate::schedule::rests`; the coordinator's log lines 22166 and 22216, and for
/// `shortfall` line 22854); once it no longer rests, and for a handed-back run with another cause
/// or with none, the row keeps the line of a run that stopped early. At the way back to
/// `on_device` the agent rests as one that was never allowed a model, and its row says so (r3
/// U7.11; these rows hand a use of a model whose table has an open row).
#[test]
fn u72a_a_run_handed_back_before_its_model_says_why_and_every_other_reads_as_today() {
    let early =
        "Last time, it stopped before the task was done, and it needs nothing from you now.";
    let ok = model_with(true, Some(true), limit(false));
    let rows = [
        (
            Tier::Frontier,
            Some(LastCause::NoAgreement),
            model(false),
            "This agent may use an AI model only after you allow it. To allow it, choose Use an AI model.",
        ),
        (
            Tier::Frontier,
            Some(LastCause::NoModelKey),
            model_with(true, Some(false), limit(false)),
            "This agent needs a key for its AI model, and no key is saved. Save a key under AI model, then run the agent again.",
        ),
        (
            Tier::Frontier,
            Some(LastCause::OverCap),
            model_with(true, Some(true), limit(true)),
            "This agent stopped at your spending limit. It waits until the limit's period ends or until you raise the limit under AI model.",
        ),
        (
            Tier::Frontier,
            Some(LastCause::Shortfall),
            ok.clone(),
            "Last time, Halvo could not do this agent's task with the AI model that this agent may use. The agent waits until you choose Start now.",
        ),
        (Tier::Frontier, Some(LastCause::NoAgreement), ok.clone(), early),
        (Tier::Frontier, Some(LastCause::NoModelKey), ok.clone(), early),
        (Tier::Frontier, Some(LastCause::OverCap), ok.clone(), early),
        (
            Tier::OnDevice,
            Some(LastCause::Shortfall),
            model(false),
            "This agent needs an AI model to do its task, so Halvo does not ask you to start it. To let it use one, choose Use an AI model.",
        ),
        (Tier::Frontier, Some(LastCause::NoBrowser), ok.clone(), early),
        (Tier::Frontier, None, ok, early),
    ];
    for (tier, cause, model, want) in rows {
        let f = u78_handed_back(tier, cause, Some(model.clone()));
        let line = status(&f, &Limits::PRODUCT, None);
        let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
        assert_eq!(r.line, want, "{tier:?} {cause:?} {model:?}");
    }
}

/// r3 U7.11: an agent of `runtime` at `tier`, with a stored playbook or with none, beside whose
/// configuration the app hands `model`, whose runs, each started by the person today, ended `ends`
/// in their order, the newest with the last cause `cause`, and behind which the hold `failures` was
/// set where `held` says so.
fn u711_agent(
    tier: Tier,
    runtime: Runtime,
    playbook: bool,
    model: Option<ModelUse>,
    ends: &[RunEnd],
    cause: Option<LastCause>,
    held: bool,
) -> Facts<Config> {
    let mut f = facts(State::Ten).remove(0);
    let mut config = Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{:032x}\",\"agent\":\"{:032x}\",\"rev\":{},\"category\":\"utility_bill_payment\",\"tier\":\"{}\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"yes\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"{}\",\"playbook\":{}}}",
        1,
        2,
        if playbook { 2 } else { 1 },
        tier.name(),
        runtime.name(),
        if playbook { "1" } else { "null" }
    ))
    .unwrap();
    config.set_model(model);
    f.config = config;
    f.events.clear();
    f.runs.clear();
    let mut whats = Vec::new();
    for (n, end) in ends.iter().enumerate() {
        let run = RunId::parse(&format!("{:032x}", 0x7b00 + n)).unwrap();
        whats.push(What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        });
        whats.push(What::RunEnded {
            run,
            cause: *end,
            defers: None,
        });
    }
    if held {
        whats.push(What::HoldSet {
            cause: HoldSetCause::Failures,
        });
    }
    for what in whats {
        f.events.push(Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.clone(),
            seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
            day: f.today,
            at: i64::from(f.today) * 86_400_000_000_000,
            what,
        });
    }
    f.last_cause = cause;
    f
}

/// r3 U7.11, THE RULE OF THE REST: an agent also rests while its stored tier is `on_device`, its
/// start would be a compile (runtime `web`, no stored playbook) and the app hands its use of a
/// model, read from its configuration alone: before any run, after a failed run and at the way back
/// from `shortfall`, whether every compile, recover and write row hands back or one is open. A tick
/// raises no `ready` for it and the person's own start passes (the rulings of r3 U7.8). With no use
/// of a model handed, with a stored playbook, for a mail agent and for an agent with its yes to a
/// hosted tier and no run, the tick raises `ready` as before.
#[test]
fn u711_an_agent_that_was_never_allowed_a_model_rests_by_its_configuration() {
    use crate::schedule::{start_step, tick_step, Around, Ask, Verdict};
    let shut = ModelUse {
        hands_back: true,
        ..model(false)
    };
    let open = model(false);
    let yes = ModelUse {
        hands_back: true,
        ..model(true)
    };
    let failed = [RunEnd::Failed];
    let back = [RunEnd::HandedBack];
    let rows = [
        (
            "on_device, web, no playbook, every compile, recover and write row hands back, no run",
            Tier::OnDevice,
            Runtime::Web,
            false,
            Some(shut.clone()),
            &[][..],
            None,
            true,
        ),
        (
            "on_device, web, no playbook, a row open, no run",
            Tier::OnDevice,
            Runtime::Web,
            false,
            Some(open.clone()),
            &[][..],
            None,
            true,
        ),
        (
            "on_device, web, no playbook, every compile, recover and write row hands back, after a failed run",
            Tier::OnDevice,
            Runtime::Web,
            false,
            Some(shut.clone()),
            &failed[..],
            Some(LastCause::StepFailed),
            true,
        ),
        (
            "on_device, web, no playbook, the way back from shortfall",
            Tier::OnDevice,
            Runtime::Web,
            false,
            Some(shut.clone()),
            &back[..],
            Some(LastCause::Shortfall),
            true,
        ),
        (
            "on_device, web, no playbook, no use of a model handed, after a failed run",
            Tier::OnDevice,
            Runtime::Web,
            false,
            None,
            &failed[..],
            Some(LastCause::StepFailed),
            false,
        ),
        (
            "on_device, web, a stored playbook, no run",
            Tier::OnDevice,
            Runtime::Web,
            true,
            Some(shut.clone()),
            &[][..],
            None,
            false,
        ),
        (
            "on_device, graph_mail, no run",
            Tier::OnDevice,
            Runtime::GraphMail,
            false,
            Some(shut.clone()),
            &[][..],
            None,
            false,
        ),
        (
            "frontier with its yes, web, no playbook, no run",
            Tier::Frontier,
            Runtime::Web,
            false,
            Some(yes),
            &[][..],
            None,
            false,
        ),
    ];
    let around = Around {
        listed: true,
        live: 0,
        model_held: false,
        queued: 0,
        window: false,
        from_queue: false,
        owed: false,
    };
    for (row, tier, runtime, playbook, model, ends, cause, rests) in rows {
        let f = u711_agent(tier, runtime, playbook, model, ends, cause, false);
        let ask = tick_step(&f, true, &Limits::PRODUCT);
        let start = start_step(&f, Trigger::Person, &Limits::PRODUCT, &around);
        println!("row: {row}: tick {ask:?}, a start of the person {start:?}");
        let want = if rests { Ask::Nothing } else { Ask::Ready };
        assert_eq!(ask, want, "{row}");
        assert!(matches!(start.verdict, Verdict::Go(_)), "{row}: {start:?}");
    }
}

/// One row of the table of the test of the row's sentences below (r3 U7.11): its label, the facts,
/// the line, the status word and the texts of the controls that the row offers.
type U711Row<'a> = (&'a str, Facts<Config>, &'a str, &'a str, &'a [&'a str]);

/// r3 U7.11, THE ROW'S SENTENCES, sign for sign (the coordinator's log line 23346): the row of a
/// resting agent with no stored playbook says S1 while the app hands that every compile, recover
/// and write row of the router's table hands back, for an agent that was never allowed a model and
/// in the place of the sentences of `no_agreement` and `shortfall`; S2 for an agent that was never
/// allowed a model where such a row is open, which then offers Use an AI model. Both have the
/// status problem. A hold stands before them, a run of a hosted agent where a row is open keeps the
/// line of its cause, and with no use of a model handed the failed run's line stands as before.
#[test]
fn u711_the_row_of_a_resting_agent_with_no_stored_playbook_says_why_by_the_fact_of_the_table() {
    let s1 =
        "This version of Halvo cannot do this agent's task, so Halvo does not ask you to start it.";
    let s2 = "This agent needs an AI model to do its task, so Halvo does not ask you to start it. To let it use one, choose Use an AI model.";
    let failed_line = "Last time, it could not finish its task.";
    let paid = "Last time, it sent a payment, and Halvo could not match the amount on the site's page afterwards.";
    let failures = "Halvo switched it off after several tries in a row did not work. To try again, choose Switch on.";
    let shortfall = "Last time, Halvo could not do this agent's task with the AI model that this agent may use. The agent waits until you choose Start now.";
    let shut = ModelUse {
        hands_back: true,
        ..model(false)
    };
    let yes_shut = ModelUse {
        hands_back: true,
        ..model(true)
    };
    let plain = ["Start now", "Switch off", "Delete"];
    let back = [
        "Start now",
        "Switch off",
        "Use this computer only",
        "Delete",
    ];
    let rows: [U711Row<'_>; 10] = [
        (
            "on_device, every compile, recover and write row hands back, no run",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(shut.clone()),
                &[],
                None,
                false,
            ),
            s1,
            "Problem",
            &plain,
        ),
        (
            "on_device, a row open, no run",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(model(false)),
                &[],
                None,
                false,
            ),
            s2,
            "Problem",
            &["Start now", "Switch off", "Use an AI model", "Delete"],
        ),
        (
            "on_device, every compile, recover and write row hands back, after a failed start of the person",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(shut.clone()),
                &[RunEnd::Failed],
                Some(LastCause::StepFailed),
                false,
            ),
            s1,
            "Problem",
            &plain,
        ),
        (
            "on_device, every compile, recover and write row hands back, three failed starts and the hold failures",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(shut.clone()),
                &[RunEnd::Failed, RunEnd::Failed, RunEnd::Failed],
                Some(LastCause::StepFailed),
                true,
            ),
            failures,
            "Problem",
            &["Start now", "Switch on", "Delete"],
        ),
        (
            "frontier with its yes, every compile, recover and write row hands back, shortfall",
            u711_agent(
                Tier::Frontier,
                Runtime::Web,
                false,
                Some(yes_shut),
                &[RunEnd::HandedBack],
                Some(LastCause::Shortfall),
                false,
            ),
            s1,
            "Problem",
            &back,
        ),
        (
            "frontier whose yes does not stand, every compile, recover and write row hands back, no_agreement",
            u711_agent(
                Tier::Frontier,
                Runtime::Web,
                false,
                Some(shut),
                &[RunEnd::HandedBack],
                Some(LastCause::NoAgreement),
                false,
            ),
            s1,
            "Problem",
            &back,
        ),
        (
            "frontier with its yes, a row open, shortfall",
            u711_agent(
                Tier::Frontier,
                Runtime::Web,
                false,
                Some(model(true)),
                &[RunEnd::HandedBack],
                Some(LastCause::Shortfall),
                false,
            ),
            shortfall,
            "Problem",
            &back,
        ),
        (
            "on_device, no use of a model handed, after a failed run",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                None,
                &[RunEnd::Failed],
                Some(LastCause::StepFailed),
                false,
            ),
            failed_line,
            "Problem",
            &plain,
        ),
        (
            "on_device, every compile, recover and write row hands back, a run that paid ended amount_differs",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(ModelUse {
                    hands_back: true,
                    ..model(false)
                }),
                &[RunEnd::Failed],
                Some(LastCause::AmountDiffers),
                false,
            ),
            paid,
            "Problem",
            &plain,
        ),
        (
            "on_device, a row open, a run that paid ended amount_differs",
            u711_agent(
                Tier::OnDevice,
                Runtime::Web,
                false,
                Some(model(false)),
                &[RunEnd::Failed],
                Some(LastCause::AmountDiffers),
                false,
            ),
            paid,
            "Problem",
            &["Start now", "Switch off", "Use an AI model", "Delete"],
        ),
    ];
    for (row, f, line, word, controls) in rows {
        let status_line = status(&f, &Limits::PRODUCT, None);
        let r = super::row(&f, status_line, NAME, None, &Limits::PRODUCT).unwrap();
        let shown: Vec<&str> = r.controls.iter().map(|c| c.text.as_str()).collect();
        println!("row: {row}: {:?} {:?} {shown:?}", r.word, r.line);
        assert_eq!(r.line, line, "{row}");
        assert_eq!(r.word, word, "{row}");
        assert_eq!(shown, controls, "{row}");
    }
}

/// The spreadsheet agent of the state `ten` (runtime file, every question answered) with `starts`
/// runs that the person started today, each but the last ended `completed` and the last ended
/// `last`, with the last cause `cause`, and the events `after` stored behind that end.
fn u323_day(starts: u32, last: RunEnd, cause: Option<LastCause>, after: &[What]) -> Facts<Config> {
    let mut f = facts(State::Ten)
        .into_iter()
        .find(|f| f.workflow.category == crate::types::Category::SpreadsheetFill)
        .unwrap();
    let mut whats = Vec::new();
    for n in 1..=starts {
        let run = RunId::parse(&format!("{:032x}", 0x7800 + n)).unwrap();
        whats.push(What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        });
        whats.push(What::RunEnded {
            run,
            cause: if n == starts { last } else { RunEnd::Completed },
            defers: None,
        });
    }
    whats.extend_from_slice(after);
    for what in whats {
        f.events.push(Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.clone(),
            seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
            day: f.today,
            at: i64::from(f.today) * 86_400_000_000_000,
            what,
        });
    }
    f.last_cause = cause;
    f
}

/// r4 U3.24 sentence 2: where an agent has starts_per_day run_started events of today, its row
/// offers the controls that it offers with one start fewer, Start now among them, with no hold
/// and with the hold `failures` (f); and its line in each state of the ruling reads the ruled
/// sentence: behind a failed run with a cause whose sentence names a new start (a) or names
/// Change its answers (b), behind answers saved after a failed run (c), behind a run that
/// finished (d), behind a failed run with no cause (e), and under the hold `failures` (f).
#[test]
fn u324_on_a_day_with_all_its_starts_used_the_row_offers_start_now_and_says_what_is_true() {
    let day = Limits::PRODUCT.starts_per_day;
    let failed = RunEnd::Failed;
    let saved = [What::ItemAnswered {
        item: ItemId::parse(&format!("{:032x}", 0x78ff)).unwrap(),
        answer: Answered::Setup(crate::types::SetupAnswer::Choose),
    }];
    let held = [What::HoldSet {
        cause: HoldSetCause::Failures,
    }];
    /// A case: its name, the last run's end and cause, the events behind that end, and the line
    /// with all the day's starts used.
    type Case<'a> = (&'a str, RunEnd, Option<LastCause>, &'a [What], &'a str);
    let rows: [Case; 6] = [
        (
            "a",
            failed,
            Some(LastCause::Changed),
            &[],
            "The spreadsheet file changed while the agent worked, so Halvo left it as it was. Choose Start now to try again.",
        ),
        (
            "b",
            failed,
            Some(LastCause::NoWorkbook),
            &[],
            "Halvo did not find the spreadsheet file where you said it is. Then choose Change its answers.",
        ),
        (
            "c",
            failed,
            Some(LastCause::Changed),
            &saved,
            "Its answers are saved. Choose Start now to try again.",
        ),
        (
            "d",
            RunEnd::Completed,
            None,
            &[],
            "Last time, it finished its task.",
        ),
        (
            "e",
            failed,
            None,
            &[],
            "Last time, it could not finish its task.",
        ),
        (
            "f",
            RunEnd::Completed,
            None,
            &held,
            "Halvo switched it off after several tries in a row did not work. To try again, choose Switch on.",
        ),
    ];
    let mut wrong = Vec::new();
    for (case, last, cause, after, want) in rows {
        let read = |starts: u32| {
            let f = u323_day(starts, last, cause, after);
            let line = status(&f, &Limits::PRODUCT, None);
            let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
            let controls: Vec<Control> = r.controls.iter().map(|c| c.control).collect();
            (r.line, controls)
        };
        let ((line, used), (_, fewer)) = (read(day), read(day - 1));
        if used != fewer || !fewer.contains(&Control::RunNow) {
            wrong.push(format!(
                "({case}) controls {used:?} with all starts used, {fewer:?} with one fewer"
            ));
        }
        if line != want {
            wrong.push(format!("({case}) reads {line:?}, not {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// r4 U3.29 sentence 2: the card's entry of a payment or an order that left says what the site's
/// page afterwards gave, from the two keys of the agents run's read on its effect, to the letter of
/// the ruled rows (the coordinator's log lines 20376, 20377 and 20492): a page that was read and
/// matched, row 1 (way (a) of the coordinator's log line 20557); the amount could not be matched,
/// row 2 with the amount that the person allowed under row 9 and the page's amount under row 10 as
/// digits, a point and two decimals; no amount found, row 11; the page could not be read, and no
/// cause, row 12; row 9's label in all but the first; each for an order in its twin's words.
#[test]
fn u329_the_card_says_what_the_page_after_a_payment_or_an_order_gave() {
    use crate::types::{Digest, EffectId, Name, Outcome, PageCause};
    let base = facts(State::Card).remove(0);
    let kept = |name: &str, value: &str| Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.into()],
    };
    let differs = PageCause::AmountDiffers { amount: Some(1334) };
    let rows = [
        (Class::Payment, Some(PageCause::AmountMatched), "It sent this payment, and the site's page afterwards showed the amount you allowed.", vec![("Pay to", "City Power"), ("Amount", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Payment, Some(differs), "It sent this payment, but Halvo could not match the amount on the site's page afterwards to the one you allowed. Check your bank or the site to see what was paid.", vec![("Pay to", "City Power"), ("Amount you allowed", "42.10 USD"), ("Amount on the site's page afterwards", "13.34"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Payment, Some(PageCause::AmountUnread), "It sent this payment, but Halvo found no amount on the site's page afterwards. Check your bank or the site to see whether it was paid, and how much.", vec![("Pay to", "City Power"), ("Amount you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Payment, Some(PageCause::PageUnread), "It sent this payment, but Halvo could not read the site's page afterwards. Check your bank or the site to see whether it was paid, and how much.", vec![("Pay to", "City Power"), ("Amount you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Payment, None, "It sent this payment, but Halvo could not read the site's page afterwards. Check your bank or the site to see whether it was paid, and how much.", vec![("Pay to", "City Power"), ("Amount you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Purchase, Some(PageCause::AmountMatched), "It sent this order, and the site's page afterwards showed the total you allowed.", vec![("Shop", "City Power"), ("Total", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Purchase, Some(differs), "It sent this order, but Halvo could not match the total on the site's page afterwards to the one you allowed. Check the shop's order page to see what was ordered, and for how much.", vec![("Shop", "City Power"), ("Total you allowed", "42.10 USD"), ("Total on the site's page afterwards", "13.34"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Purchase, Some(PageCause::AmountUnread), "It sent this order, but Halvo found no total on the site's page afterwards. Check the shop's order page to see whether it was placed, and for how much.", vec![("Shop", "City Power"), ("Total you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Purchase, Some(PageCause::PageUnread), "It sent this order, but Halvo could not read the site's page afterwards. Check the shop's order page to see whether it was placed, and for how much.", vec![("Shop", "City Power"), ("Total you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
        (Class::Purchase, None, "It sent this order, but Halvo could not read the site's page afterwards. Check the shop's order page to see whether it was placed, and for how much.", vec![("Shop", "City Power"), ("Total you allowed", "42.10 USD"), ("Pay with", "Card ending 4242"), ("Site", "site1.example")]),
    ];
    let mut wrong = Vec::new();
    for (class, page, text, fields) in rows {
        let first = if class == Class::Payment {
            "payee"
        } else {
            "seller"
        };
        let history = [RunHistory {
            run: PastRun {
                run: RunId::parse(&format!("{:032x}", 0x7901)).unwrap(),
                day: base.today,
                outcome: Outcome::Completed,
                unknown: 0,
                notes: crate::types::Notes::default(),
            },
            effects: vec![PastEffect {
                effect: EffectId::parse(&format!("{:032x}", 0x7902)).unwrap(),
                digest: Digest::parse(&"ab".repeat(32)).unwrap(),
                class,
                state: EffectState::Committed,
                cause: None,
                kept: vec![
                    kept(first, "City Power"),
                    kept("amount", "42.10 USD"),
                    kept("instrument", "Card ending 4242"),
                    kept("origin", "https://site1.example:443"),
                ],
                page,
            }],
        }];
        let c = card(&base, NAME, &history, &Limits::PRODUCT).unwrap();
        let entry = &c.runs[0].effects[0];
        let got: Vec<(&str, String, &str)> = entry
            .fields()
            .iter()
            .map(|f| (f.label.as_str(), f.value.concat(), f.from_text.as_str()))
            .collect();
        let want: Vec<(&str, String, &str)> = fields
            .iter()
            .map(|(label, value)| {
                let from = match *label {
                    "Pay to" | "Shop" | "Pay with" => "You gave this",
                    "Site" => "Set by Halvo",
                    _ => "From the site",
                };
                (*label, (*value).to_owned(), from)
            })
            .collect();
        if entry.text() != text || got != want {
            wrong.push(format!("{class} {page:?}: {:?} {got:?}", entry.text()));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The agent of the state `ten` of `category` whose newest run `run` the agent store ended
/// `failed` with the last cause `cause`, with that run's `run_started` and `run_ended` rows and
/// the events `after` behind them.
fn u329_ended(category: crate::types::Category, cause: LastCause, after: &[What]) -> Facts<Config> {
    use crate::types::Outcome;
    let mut f = facts(State::Ten).remove(0);
    f.workflow.category = category;
    let run = RunId::parse(&format!("{:032x}", 0x7a01)).unwrap();
    f.runs.insert(
        0,
        PastRun {
            run: run.clone(),
            day: f.today,
            outcome: Outcome::Failed,
            unknown: 0,
            notes: crate::types::Notes::default(),
        },
    );
    let mut whats = vec![
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run,
            cause: RunEnd::Failed,
            defers: None,
        },
    ];
    whats.extend_from_slice(after);
    for what in whats {
        f.events.push(Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.clone(),
            seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
            day: f.today,
            at: i64::from(f.today) * 86_400_000_000_000,
            what,
        });
    }
    f.last_cause = Some(cause);
    f
}

/// r4 U3.29 sentence 3: the row's and the run's lines of the three ends of a run that paid or
/// ordered, each to the letter of the ruled rows, with the class of the agent's category (way (i)
/// of the coordinator's log line 20557): a payment whose amount could not be matched, behind the
/// person's answer (the `hold_cleared` of the cause `amounts` that names the run), rows 8 and 7;
/// a page that could not be read, rows 14 and 13; a payment that was held back, rows 16 and 15;
/// each for an order in its twin's words.
#[test]
fn u329_the_row_and_the_run_say_how_a_payment_or_an_order_ended() {
    use crate::types::{Category, Cleared, Digest, EffectId, Name, PageCause};
    let answered = [What::HoldCleared {
        cause: Cleared::Amounts {
            run: RunId::parse(&format!("{:032x}", 0x7a01)).unwrap(),
        },
    }];
    let rows = [
        (Category::UtilityBillPayment, LastCause::AmountDiffers, Some(PageCause::AmountDiffers { amount: Some(1334) }),
            "Last time, it sent a payment, and Halvo could not match the amount on the site's page afterwards.",
            "It sent a payment, then stopped because Halvo could not match the amount on the site's page afterwards."),
        (Category::UtilityBillPayment, LastCause::PageUnread, Some(PageCause::PageUnread),
            "Last time, it sent a payment, but Halvo could not read the site's page afterwards.",
            "It sent a payment, then stopped because Halvo could not read the site's page afterwards."),
        (Category::UtilityBillPayment, LastCause::AmountNotSent, None,
            "Last time, it did not pay, because the payment that the site prepared named another amount than its page showed.",
            "It did not pay, because the payment that the site prepared named another amount than its page showed."),
        (Category::ShopOrder, LastCause::AmountDiffers, Some(PageCause::AmountDiffers { amount: Some(1334) }),
            "Last time, it sent an order, and Halvo could not match the total on the site's page afterwards.",
            "It sent an order, then stopped because Halvo could not match the total on the site's page afterwards."),
        (Category::ShopOrder, LastCause::PageUnread, Some(PageCause::PageUnread),
            "Last time, it sent an order, but Halvo could not read the site's page afterwards.",
            "It sent an order, then stopped because Halvo could not read the site's page afterwards."),
        (Category::ShopOrder, LastCause::AmountNotSent, None,
            "Last time, it did not order, because the order that the site prepared named another total than its page showed.",
            "It did not order, because the order that the site prepared named another total than its page showed."),
    ];
    let kept = |name: &str, value: &str| Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.into()],
    };
    let mut wrong = Vec::new();
    for (category, cause, page, row_line, run_line) in rows {
        let after: &[What] = if cause == LastCause::AmountDiffers {
            &answered
        } else {
            &[]
        };
        let f = u329_ended(category, cause, after);
        let effects = page
            .map(|page| PastEffect {
                effect: EffectId::parse(&format!("{:032x}", 0x7a02)).unwrap(),
                digest: Digest::parse(&"ab".repeat(32)).unwrap(),
                class: Class::Payment,
                state: EffectState::Committed,
                cause: None,
                kept: vec![
                    kept("payee", "City Power"),
                    kept("amount", "42.10 USD"),
                    kept("instrument", "Card ending 4242"),
                    kept("origin", "https://site1.example:443"),
                ],
                page: Some(page),
            })
            .into_iter()
            .collect();
        let history = [RunHistory {
            run: f.runs[0].clone(),
            effects,
        }];
        let line = status(&f, &Limits::PRODUCT, None);
        let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
        let c = card(&f, NAME, &history, &Limits::PRODUCT).unwrap();
        if r.line != row_line || c.runs[0].text != run_line {
            wrong.push(format!(
                "{category} {cause}: {:?} / {:?}",
                r.line, c.runs[0].text
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// r3 U11.10, the coordinator's way (b): the row of an agent of the runtime file says the note of
/// `line.note.letters_guessed` behind the line of its newest ended run, whatever its end, where
/// that run's record holds `letters_guessed`: behind the done sentence, behind the line of a
/// failed run and behind a last cause's sentence; with no key, also in a record of a build before
/// the key stood, and behind a later run without it, the row says its line as before.
#[test]
fn u1110_the_row_says_that_halvo_guessed_the_letters_behind_the_line_of_the_run_that_did() {
    use crate::types::{Failure, LastCause, Notes, Outcome, PastRun};
    let done = "Last time, it finished its task.";
    let failed = "Last time, it could not finish its task.";
    let step_failed = "It could not fill in the spreadsheet. Check the file, the sheet, the cells and the data file that you gave it. Then choose Change its answers.";
    let note = "The data file was not saved as UTF-8 text, so Halvo read its letters the way this computer writes such files. If a letter looks wrong in the spreadsheet, save the data file as CSV UTF-8 and choose Start now.";
    let guessed = Notes {
        letters_guessed: true,
    };
    let none = Notes::default();
    let base = facts(State::Ten).remove(0);
    let older = RunId::parse(&format!("{:032x}", 0x1110_0001)).unwrap();
    let newest = RunId::parse(&format!("{:032x}", 0x1110_0002)).unwrap();
    // The state, the newest run's end and record, the record of the run before it, the last
    // cause, and the line that the row says.
    type Case<'a> = (
        &'a str,
        Outcome,
        Notes,
        Option<Notes>,
        Option<&'a str>,
        String,
    );
    let cases: [Case; 7] = [
        (
            "no note",
            Outcome::Completed,
            none,
            None,
            None,
            done.to_owned(),
        ),
        (
            "the note",
            Outcome::Completed,
            guessed,
            None,
            None,
            format!("{done} {note}"),
        ),
        (
            "a later run without it",
            Outcome::Completed,
            none,
            Some(guessed),
            None,
            done.to_owned(),
        ),
        (
            "a record of a build before the key stood",
            Outcome::Completed,
            Notes::default(),
            None,
            None,
            done.to_owned(),
        ),
        (
            "a failed run whose record holds the key",
            Outcome::Failed,
            guessed,
            None,
            None,
            format!("{failed} {note}"),
        ),
        (
            "a failed run whose record holds none",
            Outcome::Failed,
            none,
            None,
            None,
            failed.to_owned(),
        ),
        (
            "a failed run with a last cause whose record holds the key",
            Outcome::Failed,
            guessed,
            None,
            Some("step_failed"),
            format!("{step_failed} {note}"),
        ),
    ];
    for (state, outcome, notes, before, cause, want) in cases {
        let mut f = base.clone();
        f.config = config(Tier::OnDevice, Runtime::File, None);
        f.store_ended = true;
        let end = match outcome {
            Outcome::Completed => RunEnd::Completed,
            _ => RunEnd::from(Failure::Failed),
        };
        let mut runs = vec![PastRun {
            run: newest.clone(),
            day: f.today,
            outcome,
            unknown: 0,
            notes,
        }];
        if let Some(notes) = before {
            runs.push(PastRun {
                run: older.clone(),
                day: f.today,
                outcome: Outcome::Completed,
                unknown: 0,
                notes,
            });
        }
        f.runs = runs;
        for what in [
            What::RunStarted {
                run: newest.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
            What::RunEnded {
                run: newest.clone(),
                cause: end,
                defers: None,
            },
        ] {
            f.events.push(Event {
                workflow: f.workflow.id.clone(),
                agent: f.agent.clone(),
                seq: std::num::NonZeroU32::MIN.saturating_add(f.events.len() as u32),
                day: f.today,
                at: i64::from(f.today) * 86_400_000_000_000,
                what,
            });
        }
        f.last_cause = cause.and_then(|c| LastCause::parse(c).ok());
        let line = status(&f, &Limits::PRODUCT, None);
        let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
        assert_eq!(r.line, want, "{state}");
    }
}
