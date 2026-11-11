use std::marker::PhantomData;

use halvo_schema::kinds::Coarse;
use halvo_schema::{
    AgentId, AppId, Category, Confidence, Domain, DomainSource, EpisodeId, Instant, KeyCounts,
    Ordinal, Part, PointerCounts, Recurrence, RequestId, SeedCategory, SiteDomain, SiteHash,
    SiteToken, SiteValue, StoreId, SummaryId, Text, Trigger, WorkflowId, WorkflowState,
};

struct Probe<T: ?Sized>(PhantomData<T>);

trait IsCoarse {
    fn coarse(&self) -> bool {
        true
    }
}

impl<T: Coarse + ?Sized> IsCoarse for Probe<T> {}

trait NotCoarse {
    fn coarse(&self) -> bool {
        false
    }
}

impl<T: ?Sized> NotCoarse for &Probe<T> {}

macro_rules! coarse {
    ($t:ty) => {
        (&Probe::<$t>(PhantomData)).coarse()
    };
}

#[test]
fn coarse_holds_for_the_field_types_of_bounded_and_kept_records_of_every_origin() {
    assert!(coarse!(WorkflowId));
    assert!(coarse!(SummaryId));
    assert!(coarse!(AgentId));
    assert!(coarse!(RequestId));
    assert!(coarse!(AppId));
    assert!(coarse!(SiteDomain));
    assert!(coarse!(SiteHash));
    assert!(coarse!(SiteValue));
    assert!(coarse!(u32));
    assert!(coarse!(Category));
    assert!(coarse!(SeedCategory));
    assert!(coarse!(WorkflowState));
    assert!(coarse!(Trigger));
    assert!(coarse!(Part));
    assert!(coarse!(DomainSource));
    assert!(coarse!(Confidence));
    assert!(coarse!(Recurrence));
    assert!(coarse!(KeyCounts));
    assert!(coarse!(PointerCounts));
    assert!(coarse!(Option<SiteValue>));
    assert!(coarse!(Vec<SummaryId>));
    assert!(coarse!(Option<Vec<u32>>));
}

#[test]
fn coarse_holds_for_no_type_of_free_text_token_instant_or_ordinal() {
    assert!(!coarse!(String));
    assert!(!coarse!(str));
    assert!(!coarse!(Text));
    assert!(!coarse!(SiteToken));
    assert!(!coarse!(Instant));
    assert!(!coarse!(Ordinal));
    assert!(!coarse!(i64));
    assert!(!coarse!(u64));
    assert!(!coarse!(f64));
    assert!(!coarse!(bool));
    assert!(!coarse!(Domain));
    assert!(!coarse!(StoreId));
    assert!(!coarse!(EpisodeId));
    assert!(!coarse!(Option<Text>));
    assert!(!coarse!(Vec<Instant>));
    assert!(!coarse!(Option<Vec<Ordinal>>));
    assert!(!coarse!(Vec<SiteToken>));
}
