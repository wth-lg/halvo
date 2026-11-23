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

/// r4 U3.17b sentence 1: a row offers Use an AI model where the app hands the agent's model (this build's table of
/// hosts holds a row and names one origin), the agent has a runtime, and its stored tier is `on_device` or its stored
/// yes does not stand; it offers Use this computer only where its stored tier is a hosted tier; both stand before
/// Delete. Where the app hands none, as with main's table, a row offers neither, whatever its tier.
#[test]
fn a_row_offers_the_two_controls_of_an_ai_model_by_the_tier_the_yes_and_the_runtime() {
    use Control::{ThisComputerOnly as Back, UseModel as Use};
    let rows = [
        (Tier::OnDevice, Runtime::Web, None, vec![]),
        (Tier::Frontier, Runtime::Web, None, vec![]),
        (Tier::OnDevice, Runtime::Web, Some(model(false)), vec![Use]),
        (Tier::OnDevice, Runtime::None, Some(model(false)), vec![]),
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
