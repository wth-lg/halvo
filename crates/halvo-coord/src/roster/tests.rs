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
