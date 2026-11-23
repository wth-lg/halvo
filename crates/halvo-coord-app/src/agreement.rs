//! The gate between the person's yes to a hosted model and a run (r4 U3.17a): an agent's agreement, as the newest
//! reply of `list_agents` hands it (r3 U7.7), stands for a hosted tier only while its triple of that tier equals the
//! row of that tier in this build's table of hosts by its bytes, and the app's start of a run names a hosted tier, so
//! that the library reads the person's key for the run, from `stands` alone. With main's data, whose table holds no
//! row, no agreement stands and no start names a hosted tier.
#![cfg(any(target_os = "macos", windows))]

use halvo_coord::types::Tier;

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

/// THE ONE FUNCTION of the gate: whether `agreement` stands for `tier`, read with the row of `tier` in this build's
/// table of hosts, `halvo_route::release::ruling().row(tier)`. It stands only where the agreement holds a triple of
/// `tier` whose origin and model word equal the origin and the model word of that row by their bytes; it does not
/// stand at a null agreement, with no triple of `tier`, where one byte differs, where the table has no row of `tier`,
/// and for `on_device`. With main's data, whose table holds no row, it stands for no agent.
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
    /// that tier, where one byte differs, where the table has no row of that tier, and for `on_device`. With main's
    /// table of hosts nothing stands.
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
        let rows: [(Option<&Agreement>, Tier, Option<(&str, &str)>, bool); 11] = [
            (Some(&both), Tier::LowCostHosted, Some((ORIGIN, SMALL)), true),
            (Some(&both), Tier::Frontier, Some((ORIGIN, LARGE)), true),
            (None, Tier::Frontier, Some((ORIGIN, LARGE)), false),
            (Some(&low), Tier::Frontier, Some((ORIGIN, LARGE)), false),
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
}
