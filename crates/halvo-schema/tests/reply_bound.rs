use halvo_schema::request::{write_reply, ANALYSIS_WRITE_MAX, INTERFACE_REQUEST_MAX, REPLY_MAX};
use halvo_schema::{EvidencePage, ReplyResult, RequestId, WorkflowsPage};

#[test]
fn the_reply_bound_is_255_kib() {
    assert_eq!(REPLY_MAX, 261_120);
    assert_eq!(REPLY_MAX, 255 * 1024);
    assert_eq!(ANALYSIS_WRITE_MAX, 65_500);
    assert_eq!(INTERFACE_REQUEST_MAX, 4_096);
}

#[test]
fn a_page_beside_a_request_id_holds_at_most_261067_bytes() {
    let workflows: WorkflowsPage = serde_json::from_str(r#"{"workflows":[],"next":null}"#).unwrap();
    let evidence: EvidencePage = serde_json::from_str(r#"{"evidence":[],"next":null}"#).unwrap();
    let id = RequestId::from_random([0x40; 16]);
    for (page, result) in [
        (
            serde_json::to_vec(&workflows).unwrap(),
            ReplyResult::Workflows(workflows),
        ),
        (
            serde_json::to_vec(&evidence).unwrap(),
            ReplyResult::Evidence(evidence),
        ),
    ] {
        let with_id = write_reply(Some(&id), Ok(&result)).unwrap();
        assert_eq!(with_id.len(), page.len() + 53);
        let null_id = write_reply(None, Ok(&result)).unwrap();
        assert_eq!(null_id.len(), page.len() + 23);
    }
    assert_eq!(REPLY_MAX - 53, 261_067);
}
