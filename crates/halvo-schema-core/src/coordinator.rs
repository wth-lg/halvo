use crate::kinds::{FieldType, KindEntry, Origin, RetentionClass};

/// The one kind of the interface run, `coord_event`, family `coord`: bounded at 93 days, of origin own, with the
/// workflow id under the key `workflow`, so that the delete of a workflow removes its rows. A row holds ids, names,
/// a day, an instant, a digest and, under `origin`, a web origin, and no text of a page, a mail, a slot or a bound
/// field. This entry does not hold that: `check_record` takes free text of up to 1,024 bytes under `origin`, which
/// the closed set of field types gives as `OwnText` for want of a type for an origin, and any name under the keys of
/// the type `Name`. The guard is the interface run's typed reader, so a `coord_event` is committed only after
/// `CoordEvent::parse` or `CoordLog::parse` of `halvo_schema::coordinator` has read it.
pub const KINDS: &[KindEntry] = &[KindEntry {
    name: "coord_event",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Own,
    family: "coord",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("agent", FieldType::Id),
        ("seq", FieldType::Count),
        ("day", FieldType::Day),
        ("at", FieldType::Instant),
        ("event", FieldType::Name),
        ("run", FieldType::Id),
        ("slot", FieldType::Day),
        ("item", FieldType::Id),
        ("kind", FieldType::Name),
        ("effect", FieldType::Id),
        ("approval", FieldType::Id),
        ("digest", FieldType::Hash),
        ("answer", FieldType::Name),
        ("cause", FieldType::Name),
        ("trigger", FieldType::Name),
        ("origin", FieldType::OwnText),
    ],
    example: r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","seq":7,"day":20767,"at":1794339420000000000,"event":"item_answered","run":null,"slot":null,"item":"7e6d5c4b3a2918070f1e2d3c4b5a6978","kind":"sign_in","effect":null,"approval":null,"digest":null,"answer":"allow","cause":null,"trigger":null,"origin":"https://northwind-power.test:443"}"#,
}];

/// The error names that the interface run's handlers answer beside the nine of the reply form: `binding`, the
/// agent is not the workflow's, and `seq`, a record's `seq` is not the stored last plus 1.
pub const ERROR_NAMES: &[&str] = &["binding", "seq"];
