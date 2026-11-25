//! The gate between the person's yes to a hosted model and a run (r4 U3.17a). An agent's agreement, as the newest
//! reply of `list_agents` hands it (r3 U7.7), stands for a hosted tier only while its triple of that tier equals the
//! row of that tier in this build's table of hosts by its bytes (`stands`). The app's start of a run names a hosted
//! tier, the one case in which the library reads the person's key for the run, from `stands` alone (`hosted`). Where
//! the table holds no row of a tier, no agreement stands for that tier and no start names it.
#![cfg(any(target_os = "macos", windows))]

use halvo_agent_client::Hosted;
use halvo_coord::types::{Tier, WorkflowId};
use serde_json::Value;

/// One triple of an agent's agreement: a hosted tier, and the origin and the model word that the person said yes to
/// for it, as the store keeps them, never trimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Triple {
    pub(crate) tier: Tier,
    pub(crate) origin: String,
    pub(crate) model: String,
}

/// An agent's agreement to a hosted model as the roster's entry hands it: the ceiling tier and its triples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Agreement {
    pub(crate) ceiling: Tier,
    pub(crate) hosts: Vec<Triple>,
}

impl Agreement {
    /// The agreement of the value of an entry's key `agreement` (r3 U7.7): `{"tier":..,"hosts":[..]}`, the ceiling
    /// tier with its triples of tier, origin and model word. Null, a missing key and a value of another shape give
    /// none.
    pub(crate) fn of(value: &Value) -> Option<Agreement> {
        let tier = |value: &Value| Tier::parse(value.as_str()?).ok();
        let hosts = value["hosts"]
            .as_array()?
            .iter()
            .map(|triple| {
                Some(Triple {
                    tier: tier(&triple["tier"])?,
                    origin: triple["origin"].as_str()?.to_owned(),
                    model: triple["model"].as_str()?.to_owned(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Agreement {
            ceiling: tier(&value["tier"])?,
            hosts,
        })
    }
}

/// What the app keeps of an agent from the newest reply of `list_agents`: its workflow, the tier of its newest
/// set-up and its agreement, none where the entry handed none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptAgent {
    pub(crate) workflow: WorkflowId,
    pub(crate) tier: Tier,
    pub(crate) agreement: Option<Agreement>,
}

/// THE ONE FUNCTION of the gate: whether `agreement` stands for `tier`, read with the row of `tier` in this build's
/// table of hosts, `halvo_route::release::ruling().row(tier)`. It stands only where the agreement holds a triple of
/// `tier` whose origin and model word equal the origin and the model word of that row by their bytes; it does not
/// stand at a null agreement, with no triple of `tier`, where one byte differs, where the table has no row of `tier`,
/// and for `on_device`.
pub(crate) fn stands(agreement: Option<&Agreement>, tier: Tier) -> bool {
    let ruling = halvo_route::release::ruling();
    let row = halvo_route::tier::Tier::parse(tier.name())
        .ok()
        .and_then(|tier| ruling.row(tier))
        .map(|row| (row.origin(), row.model()));
    stands_on(agreement, tier, row)
}

/// `stands` with the row of `tier` given as its plain origin and model word, none where the table has no row of it.
fn stands_on(agreement: Option<&Agreement>, tier: Tier, row: Option<(&str, &str)>) -> bool {
    let (Some(agreement), Some((origin, model))) = (agreement, row) else {
        return false;
    };
    tier != Tier::OnDevice
        && agreement.hosts.iter().any(|triple| {
            triple.tier == tier
                && triple.origin.as_bytes() == origin.as_bytes()
                && triple.model.as_bytes() == model.as_bytes()
        })
}

/// The library's field `hosted` of the start of a run of the agent that the newest list kept as `kept`, from `stands`
/// alone: the agent's stored tier where it is a hosted tier and its agreement stands for that tier, and none in every
/// other case, an agent that no list of this life named among them. The app's start of a run calls it, and no other
/// place of the app names a hosted tier of the library.
pub(crate) fn hosted(kept: Option<&KeptAgent>) -> Option<Hosted> {
    hosted_by(kept, stands)
}

/// `hosted` with `stands` as the caller gives it.
fn hosted_by(
    kept: Option<&KeptAgent>,
    stands: impl FnOnce(Option<&Agreement>, Tier) -> bool,
) -> Option<Hosted> {
    let kept = kept?;
    let tier = match kept.tier {
        Tier::OnDevice => return None,
        Tier::LowCostHosted => Hosted::LowCostHosted,
        Tier::Frontier => Hosted::Frontier,
    };
    stands(kept.agreement.as_ref(), kept.tier).then_some(tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triple(tier: Tier, origin: &str, model: &str) -> Triple {
        Triple {
            tier,
            origin: origin.to_owned(),
            model: model.to_owned(),
        }
    }

    const ORIGIN: &str = "https://hosted-model.test";
    const SMALL: &str = "test-model-small";
    const LARGE: &str = "test-model-large";

    /// r4 U3.17a sentence 1: an agreement stands for a hosted tier only where it holds a triple of that tier whose
    /// origin and model word equal the row of that tier by their bytes; not at a null agreement, with no triple of
    /// that tier (also where the other tier's only triple holds the row's origin and model word), where one byte
    /// differs, where the table has no row of that tier, and for `on_device`. Over this build's table of hosts an
    /// agreement whose triples name `https://hosted-model.test` stands for no tier.
    #[test]
    fn an_agreement_stands_only_where_its_triple_of_the_tier_equals_the_row_by_its_bytes() {
        let both = Agreement {
            ceiling: Tier::Frontier,
            hosts: vec![
                triple(Tier::LowCostHosted, ORIGIN, SMALL),
                triple(Tier::Frontier, ORIGIN, LARGE),
            ],
        };
        let low = Agreement {
            ceiling: Tier::LowCostHosted,
            hosts: vec![triple(Tier::LowCostHosted, ORIGIN, SMALL)],
        };
        let device = Agreement {
            ceiling: Tier::OnDevice,
            hosts: vec![triple(Tier::OnDevice, ORIGIN, SMALL)],
        };
        let other = Agreement {
            ceiling: Tier::LowCostHosted,
            hosts: vec![triple(Tier::LowCostHosted, ORIGIN, LARGE)],
        };
        let rows = [
            (
                Some(&both),
                Tier::LowCostHosted,
                Some((ORIGIN, SMALL)),
                true,
            ),
            (Some(&both), Tier::Frontier, Some((ORIGIN, LARGE)), true),
            (None, Tier::Frontier, Some((ORIGIN, LARGE)), false),
            (Some(&low), Tier::Frontier, Some((ORIGIN, LARGE)), false),
            (Some(&other), Tier::Frontier, Some((ORIGIN, LARGE)), false),
            (
                Some(&both),
                Tier::Frontier,
                Some(("https://hosted-model.tesT", LARGE)),
                false,
            ),
            (
                Some(&both),
                Tier::Frontier,
                Some(("https://hosted-model.test/", LARGE)),
                false,
            ),
            (
                Some(&both),
                Tier::Frontier,
                Some((ORIGIN, "test-model-largE")),
                false,
            ),
            (
                Some(&both),
                Tier::LowCostHosted,
                Some((ORIGIN, "test-model-small ")),
                false,
            ),
            (Some(&both), Tier::Frontier, None, false),
            (Some(&both), Tier::OnDevice, Some((ORIGIN, SMALL)), false),
            (Some(&device), Tier::OnDevice, Some((ORIGIN, SMALL)), false),
        ];
        for (agreement, tier, row, want) in rows {
            assert_eq!(
                stands_on(agreement, tier, row),
                want,
                "{agreement:?} {tier} {row:?}"
            );
        }
        for tier in [Tier::LowCostHosted, Tier::Frontier] {
            assert!(!stands(Some(&both), tier), "{tier} over release::ruling()");
        }
    }

    /// r4 U3.17a sentence 3: the start names the agent's kept tier where it is a hosted tier and the agreement of the
    /// newest list stands for it, and none in every other case: a null agreement, the other tier's triple alone (also
    /// one that holds the row's origin and model word), a row that differs, an agent that no list named, and
    /// `on_device`.
    #[test]
    fn the_start_names_the_kept_tier_only_where_the_newest_agreement_stands_for_it() {
        let low = Agreement {
            ceiling: Tier::LowCostHosted,
            hosts: vec![triple(Tier::LowCostHosted, ORIGIN, SMALL)],
        };
        let both = Agreement {
            ceiling: Tier::Frontier,
            hosts: vec![
                triple(Tier::LowCostHosted, ORIGIN, SMALL),
                triple(Tier::Frontier, ORIGIN, LARGE),
            ],
        };
        let other = Agreement {
            ceiling: Tier::LowCostHosted,
            hosts: vec![triple(Tier::LowCostHosted, ORIGIN, LARGE)],
        };
        let kept = |tier, agreement: Option<&Agreement>| {
            Some(KeptAgent {
                workflow: WorkflowId::parse(&"f1".repeat(16)).unwrap(),
                tier,
                agreement: agreement.cloned(),
            })
        };
        let rows = [
            (
                kept(Tier::LowCostHosted, Some(&low)),
                Some((ORIGIN, SMALL)),
                Some(Hosted::LowCostHosted),
            ),
            (
                kept(Tier::Frontier, Some(&both)),
                Some((ORIGIN, LARGE)),
                Some(Hosted::Frontier),
            ),
            (kept(Tier::Frontier, None), Some((ORIGIN, LARGE)), None),
            (
                kept(Tier::Frontier, Some(&low)),
                Some((ORIGIN, LARGE)),
                None,
            ),
            (
                kept(Tier::Frontier, Some(&other)),
                Some((ORIGIN, LARGE)),
                None,
            ),
            (
                kept(Tier::Frontier, Some(&both)),
                Some((ORIGIN, "test-model-other")),
                None,
            ),
            (None, Some((ORIGIN, LARGE)), None),
            (
                kept(Tier::OnDevice, Some(&both)),
                Some((ORIGIN, SMALL)),
                None,
            ),
        ];
        for (kept, row, want) in rows {
            assert_eq!(
                hosted_by(kept.as_ref(), |agreement, tier| stands_on(
                    agreement, tier, row
                )),
                want,
                "{kept:?} {row:?}"
            );
        }
    }
}
