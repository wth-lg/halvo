mod common;

use std::borrow::Borrow;
use std::ops::Deref;

use common::{assert_strict, refused, HEX_A, MAX63};
use halvo_schema::{
    AgentId, EpisodeId, ParseError, Position, RequestId, StoreId, SummaryId, WorkflowId, VERSION,
};

macro_rules! assert_not_impl {
    ($ty:ty: $($tr:path),+ $(,)?) => {
        $(
            const _: fn() = || {
                trait Ambiguous<A> {
                    fn some_item() {}
                }
                impl<T: ?Sized> Ambiguous<()> for T {}
                struct Implemented;
                impl<T: ?Sized + $tr> Ambiguous<Implemented> for T {}
                let _ = <$ty as Ambiguous<_>>::some_item;
            };
        )+
    };
}

macro_rules! assert_no_conversion {
    ($ty:ty; $($other:ty),+) => {
        assert_not_impl!($ty: $(Into<$other>),+);
        assert_not_impl!($ty:
            Into<String>, Into<Position>, Into<u64>, Into<i64>, Into<[u8; 16]>,
            From<String>, From<&'static str>, From<[u8; 16]>, From<u64>, From<i64>, From<u32>, From<Position>,
            AsRef<str>, AsRef<[u8]>, Borrow<str>, Deref,
        );
    };
}

assert_no_conversion!(WorkflowId; SummaryId, AgentId, RequestId, EpisodeId, StoreId);
assert_no_conversion!(SummaryId; WorkflowId, AgentId, RequestId, EpisodeId, StoreId);
assert_no_conversion!(AgentId; WorkflowId, SummaryId, RequestId, EpisodeId, StoreId);
assert_no_conversion!(RequestId; WorkflowId, SummaryId, AgentId, EpisodeId, StoreId);
assert_no_conversion!(EpisodeId; WorkflowId, SummaryId, AgentId, RequestId, StoreId);
assert_no_conversion!(StoreId; WorkflowId, SummaryId, AgentId, RequestId, EpisodeId);

fn rule<T: std::fmt::Debug>(result: Result<T, ParseError>) -> &'static str {
    result.expect_err("must be refused").rule()
}

macro_rules! random_id_cases {
    ($($ty:ident),+) => {
        $(
            let id = $ty::parse(HEX_A).unwrap();
            assert_eq!(id.as_str(), HEX_A);
            assert_eq!(id.to_string(), HEX_A);
            assert_eq!(rule($ty::parse(&HEX_A[..31])), "length");
            assert_eq!(rule($ty::parse(&format!("{HEX_A}0"))), "length");
            assert_eq!(rule($ty::parse("")), "length");
            assert_eq!(rule($ty::parse(&HEX_A.replace('a', "A"))), "hex_digit");
            assert_eq!(rule($ty::parse(&HEX_A.replace('a', "g"))), "hex_digit");
            assert_eq!($ty::parse("x").unwrap_err().type_name(), stringify!($ty));
            let mut bytes = [0u8; 16];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = (i as u8) * 17;
            }
            assert_eq!($ty::from_random(bytes).as_str(), "00112233445566778899aabbccddeeff");
            assert_eq!($ty::from_random([0xa5; 16]).as_str(), "a5".repeat(16));
            assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{HEX_A}\""));
            assert_eq!(serde_json::from_str::<$ty>(&format!("\"{HEX_A}\"")).unwrap(), id);
            refused::<$ty>("\"ABCDEF0123456789abcdef0123456789\"");
            refused::<$ty>("5");
            refused::<$ty>("null");
        )+
    };
}

#[test]
fn random_ids_have_32_lower_case_hex_digits() {
    random_id_cases!(WorkflowId, SummaryId, AgentId, RequestId);
}

#[test]
fn random_ids_order_and_hash_by_their_text() {
    let low = WorkflowId::parse(&"0".repeat(32)).unwrap();
    let high = WorkflowId::parse(&"f".repeat(32)).unwrap();
    assert!(low < high);
    let set: std::collections::HashSet<WorkflowId> =
        [low.clone(), low.clone(), high].into_iter().collect();
    assert_eq!(set.len(), 2);
}

#[test]
fn episode_ids_have_1_to_48_bytes_of_lower_case_letters_digits_and_hyphens() {
    for ok in ["a", "0", "-", "tier-1-stream-02-day-93", &"a".repeat(48)] {
        assert_eq!(EpisodeId::parse(ok).unwrap().as_str(), ok);
    }
    assert_eq!(rule(EpisodeId::parse("")), "length");
    assert_eq!(rule(EpisodeId::parse(&"a".repeat(49))), "length");
    for bad in ["A", "a_b", "a b", "a.b", "\u{e9}", "a/b"] {
        assert_eq!(rule(EpisodeId::parse(bad)), "character", "{bad}");
    }
    refused::<EpisodeId>("\"Upper\"");
    assert_eq!(
        serde_json::to_string(&EpisodeId::parse("e-1").unwrap()).unwrap(),
        "\"e-1\""
    );
}

#[test]
fn store_ids_have_32_lower_case_hex_digits() {
    let id = StoreId::parse(HEX_A).unwrap();
    assert_eq!(id.as_str(), HEX_A);
    assert_eq!(id.to_string(), HEX_A);
    assert_eq!(id.clone(), id);
    assert_eq!(rule(StoreId::parse(&HEX_A[1..])), "length");
    assert_eq!(rule(StoreId::parse(&HEX_A.to_uppercase())), "hex_digit");
    refused::<StoreId>("\"0123\"");
    assert_eq!(
        serde_json::from_str::<StoreId>(&format!("\"{HEX_A}\"")).unwrap(),
        id
    );
}

#[test]
fn a_parse_error_names_the_type_and_the_rule_and_never_the_input() {
    let secret = "pay-your-bill-at-northwind";
    let error = WorkflowId::parse(secret).unwrap_err();
    assert_eq!(error.type_name(), "WorkflowId");
    assert_eq!(error.rule(), "length");
    assert_eq!(error.to_string(), "WorkflowId: length");
    assert!(!format!("{error:?}").contains(secret));
    let as_dyn: &dyn std::error::Error = &error;
    assert!(!as_dyn.to_string().contains(secret));
    let read = serde_json::from_str::<WorkflowId>(&format!("\"{secret}\"")).unwrap_err();
    assert!(!read.to_string().contains(secret), "{read}");
}

#[test]
fn the_version_is_1() {
    assert_eq!(VERSION, 1);
}

#[test]
fn a_position_is_two_integers_up_to_2_pow_63_minus_1() {
    assert_strict::<Position>(r#"{"seg":4411,"idx":212}"#);
    assert_strict::<Position>(&format!(r#"{{"seg":{MAX63},"idx":{MAX63}}}"#));
    let p: Position = common::read(r#"{"idx":2,"seg":1}"#);
    assert_eq!(p, Position { seg: 1, idx: 2 });
    let over = MAX63 + 1;
    refused::<Position>(&format!(r#"{{"seg":{over},"idx":0}}"#));
    refused::<Position>(&format!(r#"{{"seg":0,"idx":{over}}}"#));
    refused::<Position>(r#"{"seg":-1,"idx":0}"#);
    refused::<Position>(r#"{"seg":1.0,"idx":0}"#);
    refused::<Position>(r#"{"seg":1e2,"idx":0}"#);
    refused::<Position>(r#"{"seg":"1","idx":0}"#);
    refused::<Position>(r#"{"seg":null,"idx":0}"#);
    assert!(serde_json::to_string(&Position { seg: over, idx: 0 }).is_err());
    assert!(serde_json::to_string(&Position {
        seg: 0,
        idx: u64::MAX
    })
    .is_err());
    assert_eq!(
        serde_json::to_string(&Position {
            seg: MAX63,
            idx: MAX63
        })
        .unwrap()
        .len(),
        53
    );
}

#[test]
fn positions_order_by_segment_then_index() {
    let a = Position { seg: 1, idx: 9 };
    let b = Position { seg: 2, idx: 0 };
    let c = Position { seg: 2, idx: 1 };
    assert!(a < b && b < c);
    let copied = a;
    assert_eq!(copied, a);
}

#[test]
fn a_position_is_refused_as_an_array() {
    common::refused_as_array::<Position>("[4411,212]");
}
