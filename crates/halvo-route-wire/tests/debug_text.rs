use halvo_route_wire::{Answer, Body, Effort, Message, Role, WireTier};

const MARKER: &str = "a private marker of the debug test, é and ü";

fn holds_no_text(debug: &str) {
    assert!(!debug.contains(MARKER), "{}", debug.len());
    assert!(!debug.contains("private marker"), "{}", debug.len());
    assert!(debug.contains(&MARKER.len().to_string()), "{debug}");
}

#[test]
fn debug_of_a_body_and_an_answer_holds_no_text() {
    let body = Body {
        tier: WireTier::Frontier,
        effort: Effort::Low,
        max_output_tokens: 1,
        messages: vec![Message {
            role: Role::User,
            content: MARKER.to_string(),
        }],
    };
    let answer = Answer::Ok {
        content: MARKER.to_string(),
        tokens_in: 1,
        tokens_out: 2,
        cost: 3,
    };
    for debug in [
        format!("{body:?}"),
        format!("{body:#?}"),
        format!("{answer:?}"),
        format!("{answer:#?}"),
        format!("{:?}", body.messages[0]),
    ] {
        holds_no_text(&debug);
    }
}
