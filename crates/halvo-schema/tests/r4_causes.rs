use std::num::NonZeroU32;

use halvo_schema::coordinator::names::{OtherKind, OtherLapse};
use halvo_schema::coordinator::record::{CoordEvent, ItemId, Lapsed, What};
use halvo_schema::kinds::check_record;
use halvo_schema::record::Instant;
use halvo_schema::{AgentId, WorkflowId};

fn row(kind: &str, cause: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","seq":9,"day":20768,"at":1794425400000000000,"event":"item_lapsed","run":null,"slot":null,"item":"7e6d5c4b3a2918070f1e2d3c4b5a6978","kind":"{kind}","effect":null,"approval":null,"digest":null,"answer":null,"cause":"{cause}","trigger":null,"origin":null}}"#
    )
}

#[test]
fn r4_causes_of_a_lapse_of_a_kind_other_than_confirm_are_five() {
    let names: Vec<&str> = OtherLapse::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        ["run_ended", "app_ended", "wait_limit", "check", "cancelled"]
    );
}

#[test]
fn r4_causes_check_and_cancelled_are_read_and_written_for_each_kind_other_than_confirm() {
    let entry = &halvo_schema::coordinator::KINDS[0];
    for kind in OtherKind::ALL {
        for (name, cause) in [
            ("check", OtherLapse::Check),
            ("cancelled", OtherLapse::Cancelled),
        ] {
            let text = row(kind.as_str(), name);
            let value = CoordEvent {
                workflow: WorkflowId::parse("0f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
                agent: AgentId::parse("1a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
                seq: NonZeroU32::new(9).unwrap(),
                day: 20768,
                at: Instant(1_794_425_400_000_000_000),
                what: What::ItemLapsed {
                    item: ItemId::parse("7e6d5c4b3a2918070f1e2d3c4b5a6978").unwrap(),
                    lapsed: Lapsed::Other { kind: *kind, cause },
                },
            };
            assert_eq!(CoordEvent::parse(text.as_bytes()).unwrap(), value, "{text}");
            assert_eq!(value.to_json().unwrap(), text);
            assert_eq!(check_record(entry, &text), Ok(()), "{text}");
        }
    }
}

#[test]
fn r4_causes_check_stays_outside_the_six_causes_of_a_confirmation() {
    let refusal = CoordEvent::parse(row("confirm", "check").as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.key(), refusal.rule()),
        (Some("cause"), "unknown_name")
    );
    assert!(CoordEvent::parse(row("confirm", "cancelled").as_bytes()).is_ok());
}
