use crate::kinds::{FieldType, KindEntry, Origin, RetentionClass};

/// The three kinds of the learning run, family `learn`, each with the workflow id under the key `workflow`:
/// `learn_run` and `learn_routine`, with no text, bounded at 93 days and of origin capture, and `learn_edit`, kept
/// and of origin own, whose `target` holds text that capture did not supply.
pub const KINDS: &[KindEntry] = &[
    KindEntry {
        name: "learn_run",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "learn",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("run", FieldType::Id),
            ("rev", FieldType::Count),
            ("day", FieldType::Day),
            ("mode", FieldType::Name),
            ("state", FieldType::Name),
            ("cause", FieldType::Name),
            ("base", FieldType::Count),
            ("assisted", FieldType::Count),
            ("kept", FieldType::Count),
            ("failed_step", FieldType::Count),
        ],
        example: r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#,
    },
    KindEntry {
        name: "learn_routine",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "learn",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("rev", FieldType::Count),
            ("day", FieldType::Day),
            ("category", FieldType::Name),
            ("site", FieldType::Site),
            ("app", FieldType::App),
            ("period", FieldType::Count),
            ("seen", FieldType::Count),
            ("first", FieldType::Day),
            ("last", FieldType::Day),
            ("next", FieldType::Day),
            ("missed", FieldType::Count),
            ("mark", FieldType::Name),
            ("since", FieldType::Day),
        ],
        example: r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#,
    },
    KindEntry {
        name: "learn_edit",
        class: RetentionClass::Kept,
        origin: Origin::Own,
        family: "learn",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("agent", FieldType::Id),
            ("edit", FieldType::Id),
            ("rev", FieldType::Count),
            ("base", FieldType::Count),
            ("n", FieldType::Count),
            ("state", FieldType::Name),
            ("cause", FieldType::Name),
            ("on", FieldType::Id),
            ("runs", FieldType::Id),
            ("first", FieldType::Day),
            ("last", FieldType::Day),
            ("clean", FieldType::Count),
            ("target", FieldType::OwnText),
        ],
        example: r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#,
    },
];

/// The error names that the learning run's handlers answer beside the nine of the reply form: `binding`, the
/// answer to a `put_playbook` of a run with no `learn_run` row.
pub const ERROR_NAMES: &[&str] = &["binding"];
