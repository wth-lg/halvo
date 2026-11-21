//! The recurrence step: a pure function over the bounded summaries that proposes one workflow for each key that
//! `K` summaries or more hold.

use std::collections::BTreeMap;

use halvo_schema::{
    AppId, Confidence, Proposal, Recurrence, RecurrenceStep, SeedCategory, SiteValue, Summary,
    SummaryRead, Trigger,
};

/// The summaries of one key that make a proposal.
pub const K: usize = 3;

/// halvo-identify's recurrence step, which a host of halvo-derived supplies to the store.
///
/// For each key with `K` summaries or more it returns one proposal, in the byte order of the keys: the category,
/// site and app of the key, with the app of the key's last summary where the key holds a site; the summaries
/// as they were visited; `count` the number of summaries, `first` and `last` their first and last day, and
/// `period` the days from the first to the last day over the number of steps between distinct days, rounded to the
/// nearest day and held within 1 to 93, or none where all lie on one day; the trigger `schedule` where a period
/// shows and `app_context` where none does; and a confidence of 20 for each summary, at most 100.
pub struct Proposer;

impl RecurrenceStep for Proposer {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        let mut groups: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
        summaries.for_each(&mut |s| {
            groups
                .entry(key_of(s.category, &s.site, &s.app))
                .or_default()
                .push(s.clone());
        });
        groups
            .into_values()
            .filter(|group| group.len() >= K)
            .filter_map(|group| proposal(&group))
            .collect()
    }
}

/// The key of a summary or a workflow as halvo-derived forms it (`key_of` in its `src/batch.rs`): the category and
/// the site key or, where the site key is null, the category and the app id. The store refuses a proposal that
/// names a summary of another key, so the two must agree.
fn key_of(category: SeedCategory, site: &Option<SiteValue>, app: &AppId) -> String {
    match site {
        Some(SiteValue::Domain { key, source }) => {
            format!("{category} {} {}", source.as_str(), key.as_str())
        }
        Some(SiteValue::Title { key }) => format!("{category} title {}", key.as_str()),
        None => format!("{category} app {}", app.as_str()),
    }
}

fn proposal(group: &[Summary]) -> Option<Proposal> {
    let head = group.first()?;
    let last_summary = group.last()?;
    let mut days: Vec<u32> = group.iter().map(|s| s.day).collect();
    days.sort_unstable();
    days.dedup();
    let first = *days.first()?;
    let last = *days.last()?;
    let steps = u32::try_from(days.len() - 1).unwrap_or(u32::MAX);
    let period = (steps > 0).then(|| ((last - first + steps / 2) / steps).clamp(1, 93));
    let count = u32::try_from(group.len()).unwrap_or(u32::MAX);
    Some(Proposal {
        category: head.category,
        site: head.site.clone(),
        app: last_summary.app.clone(),
        trigger: if period.is_some() {
            Trigger::Schedule
        } else {
            Trigger::AppContext
        },
        recurrence: Recurrence {
            count,
            period,
            first,
            last,
        },
        confidence: Confidence(count.saturating_mul(20).min(100)),
        summaries: group.iter().map(|s| s.id.clone()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use halvo_schema::{DomainSource, Part, SiteDomain, SummaryId};

    use super::*;

    struct List(Vec<Summary>);

    impl SummaryRead for List {
        fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
            for s in &self.0 {
                visit(s);
            }
        }
    }

    fn summary(n: u8, day: u32) -> Summary {
        Summary {
            id: SummaryId::from_random([n; 16]),
            category: SeedCategory::ShopOrder,
            site: Some(SiteValue::Domain {
                key: SiteDomain::parse("pinecart.test").unwrap(),
                source: DomainSource::AddressField,
            }),
            app: AppId::parse("org.chromium.Chromium").unwrap(),
            day,
            part: Part::Morning,
            minutes: 5,
            keys: None,
            pointer: None,
        }
    }

    #[test]
    fn third_summary_proposes() {
        let two = List(vec![summary(1, 20_400), summary(2, 20_407)]);
        assert!(Proposer.propose(&two).is_empty());
        let three = List(vec![
            summary(1, 20_400),
            summary(2, 20_407),
            summary(3, 20_414),
        ]);
        let proposals = Proposer.propose(&three);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].recurrence.count, 3);
        let ids: Vec<SummaryId> = (1..=3).map(|n| SummaryId::from_random([n; 16])).collect();
        assert_eq!(proposals[0].summaries, ids);
    }
}
