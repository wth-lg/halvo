#![forbid(unsafe_code)]

pub mod coordinator;
pub mod executable;
pub mod learning;
pub mod routing;

pub use halvo_schema_core::{category, ids, kinds, record, request, site};

pub use category::{Category, SeedCategory};
pub use ids::{
    AgentId, EpisodeId, ParseError, Position, RequestId, StoreId, SummaryId, WorkflowId, VERSION,
};
pub use kinds::{Coarse, FieldType, KindEntry, Origin, RetentionClass};
pub use record::{
    AddressFieldLayer, Confidence, DismissedWorkflow, Episode, Evidence, EvidencePage, FloorLayer,
    FullWorkflow, InputClass, Instant, KeyCounts, ObservedStep, Ordinal, Part, PointerCounts,
    Proposal, Recurrence, RecurrenceStep, RowRead, Summary, SummaryRead, Support, Tagged, Text,
    TextLayer, Trigger, TypedRecord, Workflow, WorkflowItem, WorkflowState, WorkflowsPage,
    WriterStep,
};
pub use request::{
    AnalysisRole, AnalysisWrite, CoordinatorBody, CoordinatorRole, CoordinatorWrite, Decision,
    DecodeError, Decoded, ErrorName, ExtBody, ExtRequest, ExtRole, InterfaceDecision,
    InterfaceRead, InterfaceRole, PushRefused, RawRequest, Read, Reply, ReplyResult, Role,
    RunnerBody, RunnerRequest, RunnerRole,
};
pub use site::{
    AppId, Domain, DomainSource, ObservedSite, SiteDomain, SiteHash, SiteToken, SiteValue,
};
