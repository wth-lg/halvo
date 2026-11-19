mod common;

use common::stub::{Seen, Stub};
use halvo_connect::{
    reply_senders, AnswerFault, Calendar, CallError, EventCounts, Mail, MailCounts, ReplyRule,
    RunDay, Token,
};
use halvo_schema::executable::names::YesNo;
use halvo_schema::record::Instant;
use serde_json::{json, Value};

const BEARER: &str = "Bearer stub-token-1";
const PREFER: &str = "IdType=\"ImmutableId\"";
const SELECT: &str = "$top=25&$select=id%2CisRead%2Cfrom%2CtoRecipients%2CreplyTo%2CreceivedDateTime%2CconversationId%2Csubject%2Cbody%2CisReadReceiptRequested";
const ANN: &str = "/v1.0/me/mailFolders/inbox/messages?$filter=isRead%20eq%20false%20and%20from%2FemailAddress%2Faddress%20eq%20%27ann%40people.test%27%20and%20receivedDateTime%20ge%202026-11-12T00%3A00%3A00Z&$top=25&$select=id%2CisRead%2Cfrom%2CtoRecipients%2CreplyTo%2CreceivedDateTime%2CconversationId%2Csubject%2Cbody%2CisReadReceiptRequested";
const WINDOW: &str = "/v1.0/me/calendarView?startDateTime=2026-10-13T00%3A00%3A00Z&endDateTime=2026-11-13T00%3A00%3A00Z&$top=25&$select=id%2Csubject%2Cstart%2CisCancelled%2CresponseStatus%2CisOrganizer";
const NOV_12_2026: u64 = 1_794_441_600;

type SetMail = fn(&mut MailCounts);
type SetEvent = fn(&mut EventCounts);
const SECOND: i64 = 1_000_000_000;

fn token() -> Token {
    Token::new("stub-token-1".to_owned()).expect("a token")
}

fn seen(path: &str) -> Seen {
    Seen {
        method: "GET".to_owned(),
        path: path.to_owned(),
        authorization: Some(BEARER.to_owned()),
        prefer: Some(PREFER.to_owned()),
        body: None,
    }
}

fn rule(slot: &str) -> ReplyRule {
    ReplyRule::new(
        "me@home.test".to_owned(),
        reply_senders(slot).expect("senders"),
        Instant(NOV_12_2026 as i64 * SECOND + SECOND - 1),
    )
}

fn message(id: &str, from: &str) -> Value {
    json!({
        "id": id,
        "conversationId": format!("c-{id}"),
        "subject": format!("s-{id}"),
        "isRead": false,
        "from": {"emailAddress": {"address": from}},
        "toRecipients": [{"emailAddress": {"address": "me@home.test"}}],
        "replyTo": [],
        "receivedDateTime": "2026-11-12T00:00:00Z",
        "isReadReceiptRequested": false,
        "body": {"content": format!("b-{id}")},
    })
}

fn event(id: &str, cancelled: bool) -> Value {
    json!({
        "id": id,
        "subject": format!("s-{id}"),
        "start": {"dateTime": "2026-11-01T10:00:00.0000000", "timeZone": "UTC"},
        "isCancelled": cancelled,
        "responseStatus": {"response": "accepted"},
        "isOrganizer": false,
    })
}

fn page(items: Vec<Value>, link: Option<String>) -> String {
    let mut page = json!({ "value": items });
    if let Some(link) = link {
        page["@odata.nextLink"] = json!(link);
    }
    page.to_string()
}

fn mail_counts(cut: YesNo, more_pages: YesNo) -> MailCounts {
    MailCounts {
        before_start: 0,
        cut,
        no_key: 0,
        not_to_account: 0,
        incomplete: 0,
        reply_to_other: 0,
        more_pages,
        earlier_page: 0,
    }
}

fn event_counts(cut: YesNo, more_pages: YesNo) -> EventCounts {
    EventCounts {
        cut,
        no_key: 0,
        organized: 0,
        incomplete: 0,
        more_pages,
        earlier_page: 0,
    }
}

fn mail_ids(stub: &Stub, slot: &str) -> Result<(Vec<String>, MailCounts), CallError> {
    stub.graph("mail")
        .mail_list(&token(), &rule(slot))
        .map(|list| {
            (
                list.iter().map(|m| m.id().to_owned()).collect(),
                list.counts(),
            )
        })
}

fn event_ids(stub: &Stub) -> Result<(Vec<String>, EventCounts), CallError> {
    stub.graph("calendar")
        .calendar_list(&token(), RunDay::from_unix_seconds(NOV_12_2026))
        .map(|list| {
            (
                list.iter().map(|e| e.id().to_owned()).collect(),
                list.counts(),
            )
        })
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn each_list_request_names_its_filter_page_size_and_fields_and_carries_the_prefer_header() {
    let quoted = "/v1.0/me/mailFolders/inbox/messages?$filter=isRead%20eq%20false%20and%20from%2FemailAddress%2Faddress%20eq%20%27o%27%27neil%40people.test%27%20and%20receivedDateTime%20ge%202026-11-12T00%3A00%3A00Z&";
    let quoted = format!("{quoted}{SELECT}");
    let empty = page(Vec::new(), None);
    let stub = Stub::start(vec![
        (format!("GET {ANN}"), 200, empty.clone()),
        (format!("GET {quoted}"), 200, empty.clone()),
        (format!("GET {WINDOW}"), 200, empty),
    ]);
    assert_eq!(
        mail_ids(&stub, "ann@people.test, o'neil@people.test"),
        Ok((Vec::new(), mail_counts(YesNo::No, YesNo::No)))
    );
    assert_eq!(
        event_ids(&stub),
        Ok((Vec::new(), event_counts(YesNo::No, YesNo::No)))
    );
    assert_eq!(stub.seen(), [seen(ANN), seen(&quoted), seen(WINDOW)]);
}

#[test]
fn a_chain_follows_its_links_in_either_spelling_and_counts_an_id_that_an_earlier_page_gave() {
    let two = "/v1.0/me/mailFolders('inbox')/messages?$skip=1";
    let three = "/V1.0/ME/MAILFOLDERS/INBOX/MESSAGES?$skip=2";
    let stub = Stub::start_at(|port| {
        vec![
            (
                format!("GET {ANN}"),
                200,
                page(
                    vec![message("m1", "ann@people.test")],
                    Some(format!("http://mail.agent.test:{port}{two}")),
                ),
            ),
            (
                format!("GET {two}"),
                200,
                page(
                    vec![
                        message("m1", "ann@people.test"),
                        message("m2", "ann@people.test"),
                    ],
                    Some(format!("http://MAIL.agent.test:{port}{three}")),
                ),
            ),
            (
                format!("GET {three}"),
                200,
                page(vec![message("m3", "ann@people.test")], None),
            ),
        ]
    });
    let mut counts = mail_counts(YesNo::No, YesNo::No);
    counts.earlier_page = 1;
    assert_eq!(
        mail_ids(&stub, "ann@people.test"),
        Ok((ids(&["m1", "m2", "m3"]), counts))
    );
    assert_eq!(stub.seen(), [seen(ANN), seen(two), seen(three)]);
}

#[test]
fn an_id_that_an_earlier_chain_gave_is_counted_in_earlier_page() {
    let bob = ANN.replace("ann%40", "bob%40");
    let stub = Stub::start(vec![
        (
            format!("GET {ANN}"),
            200,
            page(vec![message("m1", "bob@people.test")], None),
        ),
        (
            format!("GET {bob}"),
            200,
            page(
                vec![
                    message("m1", "bob@people.test"),
                    message("m2", "bob@people.test"),
                ],
                None,
            ),
        ),
    ]);
    let mut counts = mail_counts(YesNo::No, YesNo::No);
    counts.earlier_page = 1;
    assert_eq!(
        mail_ids(&stub, "ann@people.test, bob@people.test"),
        Ok((ids(&["m1", "m2"]), counts))
    );
}

#[test]
fn a_calendar_id_that_an_earlier_page_gave_is_counted_in_earlier_page() {
    let two = "/v1.0/me/calendarview?$skiptoken=x";
    let stub = Stub::start_at(|port| {
        vec![
            (
                format!("GET {WINDOW}"),
                200,
                page(
                    vec![event("e1", true)],
                    Some(format!("http://calendar.agent.test:{port}{two}")),
                ),
            ),
            (
                format!("GET {two}"),
                200,
                page(vec![event("e1", true), event("e2", true)], None),
            ),
        ]
    });
    let mut counts = event_counts(YesNo::No, YesNo::No);
    counts.earlier_page = 1;
    assert_eq!(event_ids(&stub), Ok((ids(&["e1", "e2"]), counts)));
}

#[test]
fn a_link_of_another_place_fails_the_call_and_no_request_is_sent_to_it() {
    let links = |port: u16| {
        vec![
            json!(format!(
                "http://evil.agent.test:{port}/v1.0/me/mailFolders/inbox/messages"
            )),
            json!(format!(
                "http://mail.agent.test:{}/v1.0/me/mailFolders/inbox/messages",
                port + 1
            )),
            json!(format!(
                "https://mail.agent.test:{port}/v1.0/me/mailFolders/inbox/messages"
            )),
            json!(format!("http://mail.agent.test:{port}/v1.0/me/messages")),
            json!(format!(
                "http://mail.agent.test:{port}/v1.0/me/mailFolders/drafts/messages"
            )),
            json!(format!(
                "http://mail.agent.test:{port}/v1.0/me/mailFolders/inbox/messages/x"
            )),
            json!(format!(
                "http://mail.agent.test:{port}/v1.0/users/x/mailFolders/inbox/messages"
            )),
            json!(format!(
                "http://u@mail.agent.test:{port}/v1.0/me/mailFolders/inbox/messages"
            )),
            json!(format!(
                "http://u:p@mail.agent.test:{port}/v1.0/me/mailFolders/inbox/messages"
            )),
            json!(format!(
                "http://mail.agent.test:{port}/v1.0/me/mailFolders/inbox/messages#x"
            )),
            json!("/v1.0/me/mailFolders/inbox/messages?$skip=1"),
        ]
    };
    for index in 0..11 {
        let stub = Stub::start_at(|port| {
            let mut first = json!({"value": [message("m1", "ann@people.test")]});
            first["@odata.nextLink"] = links(port)[index].clone();
            vec![(format!("GET {ANN}"), 200, first.to_string())]
        });
        assert_eq!(
            mail_ids(&stub, "ann@people.test"),
            Err(CallError::Answer(AnswerFault::Link)),
            "link {index}"
        );
        assert_eq!(stub.seen(), [seen(ANN)], "link {index}");
    }
    let stub = Stub::start(vec![(
        format!("GET {ANN}"),
        200,
        json!({"value": [], "@odata.nextLink": 5}).to_string(),
    )]);
    assert_eq!(
        mail_ids(&stub, "ann@people.test"),
        Err(CallError::Answer(AnswerFault::Form))
    );
}

#[test]
fn a_page_with_more_items_than_asked_for_fails_the_call() {
    for (count, ok) in [(25, true), (26, false)] {
        let items = (0..count).map(|n| event(&format!("e{n}"), false)).collect();
        let stub = Stub::start(vec![(format!("GET {WINDOW}"), 200, page(items, None))]);
        let got = event_ids(&stub);
        if ok {
            assert!(got.is_ok(), "{count}");
        } else {
            assert_eq!(got, Err(CallError::Answer(AnswerFault::Page)), "{count}");
        }
    }
}

#[test]
fn one_id_twice_on_one_page_or_an_id_that_a_path_would_lose_fails_the_call() {
    let stub = Stub::start(vec![(
        format!("GET {WINDOW}"),
        200,
        page(
            vec![event("e1", false), event("e2", true), event("e1", false)],
            None,
        ),
    )]);
    assert_eq!(
        event_ids(&stub),
        Err(CallError::Answer(AnswerFault::Repeated))
    );
    for id in ["", ".", ".."] {
        let stub = Stub::start(vec![(
            format!("GET {ANN}"),
            200,
            page(vec![message(id, "eve@people.test")], None),
        )]);
        assert_eq!(
            mail_ids(&stub, "ann@people.test"),
            Err(CallError::Answer(AnswerFault::Id)),
            "{id}"
        );
    }
}

fn chain_of_pages(
    first: &str,
    host: &'static str,
    pages: Vec<Vec<Value>>,
    last_link: bool,
) -> Stub {
    let first = first.to_owned();
    Stub::start_at(move |port| {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(n, items)| {
                let path = if n == 0 {
                    first.clone()
                } else {
                    format!("{}?$skip={n}", first.split_once('?').expect("a query").0)
                };
                let link = (n + 1 < count || last_link).then(|| {
                    format!(
                        "http://{host}.agent.test:{port}{}?$skip={}",
                        first.split_once('?').expect("a query").0,
                        n + 1
                    )
                });
                (format!("GET {path}"), 200, page(items, link))
            })
            .collect()
    })
}

#[test]
fn a_calendar_chain_of_ten_pages_with_a_link_to_an_eleventh_gives_more_pages_and_no_cut() {
    for eleventh in [Vec::new(), vec![event("e-11", true)]] {
        let mut pages: Vec<Vec<Value>> = (0..10)
            .map(|n| vec![event(&format!("e{n}"), false)])
            .collect();
        pages.push(eleventh);
        let stub = chain_of_pages(WINDOW, "calendar", pages, false);
        assert_eq!(
            event_ids(&stub),
            Ok((Vec::new(), event_counts(YesNo::No, YesNo::Yes)))
        );
        assert_eq!(stub.seen().len(), 10, "the eleventh page is not read");
    }
}

#[test]
fn a_mail_chain_of_ten_pages_with_a_link_gives_more_pages_and_no_cut() {
    let pages: Vec<Vec<Value>> = (0..11)
        .map(|n| vec![message(&format!("m{n}"), "eve@people.test")])
        .collect();
    let stub = chain_of_pages(ANN, "mail", pages, false);
    assert_eq!(
        mail_ids(&stub, "ann@people.test"),
        Ok((Vec::new(), mail_counts(YesNo::No, YesNo::Yes)))
    );
    assert_eq!(stub.seen().len(), 10);
}

#[test]
fn a_chain_of_nine_pages_whose_last_has_no_link_gives_neither_fact() {
    let pages: Vec<Vec<Value>> = (0..9)
        .map(|n| vec![event(&format!("e{n}"), true)])
        .collect();
    let stub = chain_of_pages(WINDOW, "calendar", pages, false);
    let (kept, counts) = event_ids(&stub).expect("an answer");
    assert_eq!(kept.len(), 9);
    assert_eq!(counts, event_counts(YesNo::No, YesNo::No));
}

#[test]
fn twenty_selected_events_before_the_end_of_the_tenth_page_give_the_cut_and_no_more_pages() {
    let mut pages: Vec<Vec<Value>> = (0..9)
        .map(|n| vec![event(&format!("a{n}"), true), event(&format!("b{n}"), true)])
        .collect();
    pages.push(vec![
        event("a9", true),
        event("b9", true),
        event("c9", true),
    ]);
    let stub = chain_of_pages(WINDOW, "calendar", pages, true);
    let (kept, counts) = event_ids(&stub).expect("an answer");
    assert_eq!(kept.len(), 20);
    assert_eq!(kept.last().map(String::as_str), Some("b9"));
    assert_eq!(counts, event_counts(YesNo::Yes, YesNo::No));
    let mut pages: Vec<Vec<Value>> = (0..4)
        .map(|n| (0..5).map(|k| event(&format!("e{n}-{k}"), true)).collect())
        .collect();
    pages.push(vec![event("e4-0", true)]);
    let stub = chain_of_pages(WINDOW, "calendar", pages, false);
    assert_eq!(
        event_ids(&stub).map(|(kept, counts)| (kept.len(), counts)),
        Ok((20, event_counts(YesNo::Yes, YesNo::No)))
    );
    assert_eq!(
        stub.seen().len(),
        4,
        "no page is read once the bound is held"
    );
}

#[test]
fn the_bound_met_at_the_last_item_of_the_tenth_page_with_a_link_left_gives_the_cut_and_no_more_pages(
) {
    let pages: Vec<Vec<Value>> = (0..10)
        .map(|n| vec![event(&format!("a{n}"), true), event(&format!("b{n}"), true)])
        .collect();
    let stub = chain_of_pages(WINDOW, "calendar", pages, true);
    assert_eq!(
        event_ids(&stub).map(|(kept, counts)| (kept.len(), counts)),
        Ok((20, event_counts(YesNo::Yes, YesNo::No)))
    );
    let pages: Vec<Vec<Value>> = (0..10)
        .map(|n| vec![message(&format!("m{n}"), "ann@people.test")])
        .collect();
    let stub = chain_of_pages(ANN, "mail", pages, true);
    assert_eq!(
        mail_ids(&stub, "ann@people.test").map(|(kept, counts)| (kept.len(), counts)),
        Ok((10, mail_counts(YesNo::Yes, YesNo::No)))
    );
}

#[test]
fn eleven_selected_messages_give_ten_and_the_cut_and_ten_give_no_cut() {
    let eleven: Vec<Value> = (0..11)
        .map(|n| message(&format!("m{n}"), "ann@people.test"))
        .collect();
    let stub = Stub::start(vec![(
        format!("GET {ANN}"),
        200,
        page(eleven.clone(), None),
    )]);
    let (kept, counts) = mail_ids(&stub, "ann@people.test").expect("an answer");
    assert_eq!(kept.len(), 10);
    assert_eq!(counts, mail_counts(YesNo::Yes, YesNo::No));
    let stub = Stub::start(vec![(
        format!("GET {ANN}"),
        200,
        page(eleven[..10].to_vec(), None),
    )]);
    let (kept, counts) = mail_ids(&stub, "ann@people.test").expect("an answer");
    assert_eq!(kept.len(), 10);
    assert_eq!(counts, mail_counts(YesNo::No, YesNo::No));
    let bob = ANN.replace("ann%40", "bob%40");
    let stub = Stub::start(vec![
        (format!("GET {ANN}"), 200, page(eleven[..10].to_vec(), None)),
        (format!("GET {bob}"), 200, page(Vec::new(), None)),
    ]);
    let (kept, counts) = mail_ids(&stub, "ann@people.test, bob@people.test").expect("an answer");
    assert_eq!(kept.len(), 10);
    assert_eq!(counts, mail_counts(YesNo::Yes, YesNo::No));
    assert_eq!(
        stub.seen(),
        [seen(ANN)],
        "the chain of a later sender is not read"
    );
}

#[test]
fn one_message_of_each_kind_left_out_gives_1_for_its_kind_and_0_for_the_others() {
    let with = |pointer: &str, value: Value| {
        let mut item = message("m1", "ann@people.test");
        *item.pointer_mut(pointer).expect("the key") = value;
        item
    };
    let mut no_id = message("m1", "ann@people.test");
    no_id.as_object_mut().expect("an object").remove("id");
    let mut no_conversation = message("m1", "ann@people.test");
    no_conversation
        .as_object_mut()
        .expect("an object")
        .remove("conversationId");
    let rows: Vec<(Value, SetMail)> = vec![
        (no_id, |c| c.no_key = 1),
        (
            with("/receivedDateTime", json!("2026-11-11T23:59:59Z")),
            |c| c.before_start = 1,
        ),
        (
            with("/toRecipients/0/emailAddress/address", json!("x@home.test")),
            |c| c.not_to_account = 1,
        ),
        (no_conversation, |c| c.incomplete = 1),
        (
            with(
                "/replyTo",
                json!([{"emailAddress": {"address": "eve@people.test"}}]),
            ),
            |c| c.reply_to_other = 1,
        ),
        (with("/isRead", json!(true)), |_| {}),
        (
            with("/from/emailAddress/address", json!("eve@people.test")),
            |_| {},
        ),
    ];
    for (item, set) in rows {
        let stub = Stub::start(vec![(
            format!("GET {ANN}"),
            200,
            page(vec![item.clone()], None),
        )]);
        let mut want = mail_counts(YesNo::No, YesNo::No);
        set(&mut want);
        assert_eq!(
            mail_ids(&stub, "ann@people.test"),
            Ok((Vec::new(), want)),
            "{item}"
        );
    }
}

#[test]
fn one_event_of_each_kind_left_out_gives_1_for_its_kind_and_0_for_the_others() {
    let with = |key: &str, value: Value| {
        let mut item = event("e1", true);
        item[key] = value;
        item
    };
    let mut no_key = event("e1", true);
    no_key
        .as_object_mut()
        .expect("an object")
        .remove("isOrganizer");
    let mut organized_live = with("isOrganizer", json!(true));
    organized_live["isCancelled"] = json!(false);
    let rows: Vec<(Value, SetEvent)> = vec![
        (no_key, |c| c.no_key = 1),
        (with("isOrganizer", json!(true)), |c| c.organized = 1),
        (with("subject", json!(1)), |c| c.incomplete = 1),
        (organized_live, |_| {}),
    ];
    for (item, set) in rows {
        let stub = Stub::start(vec![(
            format!("GET {WINDOW}"),
            200,
            page(vec![item.clone()], None),
        )]);
        let mut want = event_counts(YesNo::No, YesNo::No);
        set(&mut want);
        assert_eq!(event_ids(&stub), Ok((Vec::new(), want)), "{item}");
    }
}

#[test]
fn the_same_events_are_selected_in_either_order_of_the_view_below_the_bound() {
    let events: Vec<Value> = (0..12)
        .map(|n| event(&format!("e{n}"), n % 3 == 0))
        .collect();
    let mut sets = Vec::new();
    for order in [events.clone(), events.into_iter().rev().collect::<Vec<_>>()] {
        let pages: Vec<Vec<Value>> = order.chunks(5).map(<[Value]>::to_vec).collect();
        let stub = chain_of_pages(WINDOW, "calendar", pages, false);
        let (mut kept, counts) = event_ids(&stub).expect("an answer");
        assert_eq!(counts, event_counts(YesNo::No, YesNo::No));
        kept.sort();
        sets.push(kept);
    }
    assert_eq!(sets[0], ids(&["e0", "e3", "e6", "e9"]));
    assert_eq!(sets[0], sets[1]);
}

#[test]
fn a_calendar_link_to_another_collection_fails_the_call_and_no_request_is_sent_to_it() {
    for path in [
        "/v1.0/me/events",
        "/v1.0/me/mailFolders/inbox/messages",
        "/v1.0/me/calendarView/x",
    ] {
        let stub = Stub::start_at(|port| {
            vec![
                (
                    format!("GET {WINDOW}"),
                    200,
                    page(
                        vec![event("e1", true)],
                        Some(format!("http://calendar.agent.test:{port}{path}?$skip=1")),
                    ),
                ),
                (format!("GET {path}?$skip=1"), 200, page(Vec::new(), None)),
            ]
        });
        assert_eq!(
            event_ids(&stub),
            Err(CallError::Answer(AnswerFault::Link)),
            "{path}"
        );
        assert_eq!(stub.seen(), [seen(WINDOW)], "{path}");
    }
}

#[test]
fn a_next_link_with_a_password_and_no_user_fails_the_call_and_no_request_is_sent_to_it() {
    let next = "/v1.0/me/mailFolders/inbox/messages?$skip=1";
    let stub = Stub::start_at(|port| {
        vec![
            (
                format!("GET {ANN}"),
                200,
                page(
                    Vec::new(),
                    Some(format!("http://:p@mail.agent.test:{port}{next}")),
                ),
            ),
            (format!("GET {next}"), 200, page(Vec::new(), None)),
        ]
    });
    assert_eq!(
        mail_ids(&stub, "ann@people.test"),
        Err(CallError::Answer(AnswerFault::Link))
    );
    assert_eq!(stub.seen(), [seen(ANN)]);
}

#[test]
fn an_event_that_the_person_has_not_declined_is_not_kept_counted_or_deleted() {
    let not_declined = |id: &str, response: &str| {
        let mut item = event(id, false);
        item["responseStatus"] = json!({"response": response});
        item
    };
    let stub = Stub::start(vec![(
        format!("GET {WINDOW}"),
        200,
        page(
            vec![
                not_declined("e1", "notResponded"),
                not_declined("e2", "none"),
                not_declined("e3", "tentativelyAccepted"),
                not_declined("e4", "accepted"),
            ],
            None,
        ),
    )]);
    let events = stub
        .graph("calendar")
        .calendar_list(&token(), RunDay::from_unix_seconds(NOV_12_2026))
        .expect("calendar_list answers");
    assert_eq!(events.len(), 0, "no event is kept");
    assert_eq!(
        events.counts(),
        event_counts(YesNo::No, YesNo::No),
        "none is counted"
    );
    assert_eq!(
        events.into_deletes().count(),
        0,
        "no event can get a DELETE"
    );
    assert_eq!(stub.seen(), [seen(WINDOW)], "the GET alone");
}
