use crate::kinds::{FieldType, KindEntry, Origin, RetentionClass};

/// The error names that this run's handlers answer, in the reply form of the workflow run.
pub const ERROR_NAMES: &[&str] = &["binding", "playbook", "config", "class", "length", "effect"];

/// The two kinds of origin capture of this run, family `agent`: `agent_config`, kept, and `agent_run`, bounded at
/// 93 days. The five kinds of origin own come with their own entries.
pub const KINDS: &[KindEntry] = &[
    KindEntry {
        name: "agent_config",
        class: RetentionClass::Kept,
        origin: Origin::Capture,
        family: "agent",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("rev", FieldType::Count),
            ("category", FieldType::Name),
            ("tier", FieldType::Name),
            ("reasoning", FieldType::Name),
            ("autonomy", FieldType::Name),
            ("allow", FieldType::Name),
            ("caps", FieldType::Name),
            ("limits", FieldType::Count),
            ("budget", FieldType::Count),
            ("budget", FieldType::Name),
            ("permissions", FieldType::Name),
            ("runtime", FieldType::Name),
            ("playbook", FieldType::Count),
        ],
        example: r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#,
    },
    KindEntry {
        name: "agent_run",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "agent",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("run", FieldType::Id),
            ("day", FieldType::Day),
            ("mode", FieldType::Name),
            ("outcome", FieldType::Name),
            ("playbook", FieldType::Count),
            ("steps", FieldType::Count),
            ("prompts", FieldType::Count),
            ("handbacks", FieldType::Count),
            ("confirmations", FieldType::Count),
            ("effects", FieldType::Count),
            ("unknown", FieldType::Count),
            ("model_calls", FieldType::Count),
            ("tokens_in", FieldType::Count),
            ("tokens_out", FieldType::Count),
            ("wall_s", FieldType::Count),
            ("model", FieldType::Name),
        ],
        example: r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","run":"50000000000000000000000000000001","day":20768,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":1,"handbacks":0,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":14,"model":null}"#,
    },
];
