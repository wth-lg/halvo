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
        },
    }
}

/// r4 U3.17b sentence 1, as the coordinator's ruling of log line 18496 turns it: a row offers Use an AI model where
/// the app hands the agent's model (this build's table of hosts holds a row and names one origin), the agent's runtime
/// asks a model (`MODEL_RUNTIMES`), and its stored tier is `on_device` or its stored yes does not stand; it offers Use
/// this computer only where its stored tier is a hosted tier; both stand before Delete. Where the app hands none, as
/// with main's table, a row offers neither, whatever its tier; a file agent in a build with a host is offered nothing.
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
            "Halvo cannot read the data file. Each line must be one row with commas between its values, and every row as long as the first. Then choose Change its answers.",
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
