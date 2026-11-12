use halvo_schema::kinds::declared_error_names;
use halvo_schema::request::{decode_reply, write_reply, DecodeError, ErrorName, ReplyResult};
use halvo_schema::RequestId;

const ID: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

const NINE: [&str; 9] = [
    "malformed",
    "version",
    "role",
    "too_large",
    "store",
    "no_workflow",
    "state",
    "unsupported",
    "internal",
];

fn id() -> RequestId {
    RequestId::parse(ID).expect("a request id")
}

fn json(text: &str) -> String {
    serde_json::to_string(text).expect("a JSON string")
}

fn error_reply(id: Option<&RequestId>, name: &str) -> String {
    let id = id.map_or("null".to_owned(), |id| json(id.as_str()));
    format!(r#"{{"v":1,"id":{id},"error":{}}}"#, json(name))
}

fn form_faults<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    names
        .into_iter()
        .filter(|name| {
            !((1..=32).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
        })
        .map(|name| {
            format!("the error name {name:?} has not 1 to 32 bytes of a to z, 0 to 9 and _")
        })
        .collect()
}

#[test]
fn a_reply_is_written_and_read_back_with_each_name_that_a_file_of_a_later_run_declares() {
    let declared: Vec<&'static str> = declared_error_names().collect();
    assert!(
        !declared.is_empty(),
        "no file of a later run declares an error name, so this test reads nothing"
    );
    let id = id();
    for name in declared {
        let error = ErrorName::read(name).expect("a declared name");
        assert_eq!(error.as_str(), name);
        match ErrorName::parse(name) {
            Ok(nine) => assert_eq!(error, nine, "{name}"),
            Err(_) => assert!(matches!(error, ErrorName::Declared(_)), "{name}"),
        }
        for id in [Some(&id), None] {
            let bytes = write_reply(id, Err(error)).expect("an error reply is always written");
            assert_eq!(
                String::from_utf8(bytes.clone()).unwrap(),
                error_reply(id, name)
            );
            let reply = decode_reply(&bytes).expect("the reply is read back");
            assert_eq!(reply.id.as_ref(), id);
            assert_eq!(reply.outcome.expect_err("an error reply"), error);
        }
        assert_eq!(serde_json::to_string(&error).unwrap(), json(name));
        assert_eq!(
            serde_json::from_str::<ErrorName>(&json(name)).unwrap(),
            error
        );
        assert_eq!(error.to_string(), name);
    }
}

#[test]
fn a_declared_name_holds_the_string_of_the_list_that_declares_it() {
    for name in declared_error_names().filter(|name| ErrorName::parse(name).is_err()) {
        let copy = name.to_owned();
        let ErrorName::Declared(held) = ErrorName::read(&copy).expect("a declared name") else {
            panic!("{name} is read as one of the nine");
        };
        assert!(
            declared_error_names().any(|listed| std::ptr::eq(listed, held.as_str())),
            "{name}: the name that ErrorName holds is no string of a list ERROR_NAMES"
        );
    }
}

#[test]
fn a_reply_with_a_name_that_no_file_declares_is_malformed_and_no_error_name_holds_that_name() {
    let id = id();
    let long = "x".repeat(33);
    let undeclared = [
        "nobody_declares_this",
        "Binding",
        "BINDING",
        "binding ",
        " binding",
        "bindin",
        "bindingg",
        "bind-ing",
        "Malformed",
        "malformed ",
        "",
        long.as_str(),
        "binding\u{0}",
    ];
    for name in undeclared {
        assert!(declared_error_names().all(|d| d != name), "{name:?}");
        assert!(!NINE.contains(&name), "{name:?}");
        for id in [Some(&id), None] {
            assert_eq!(
                decode_reply(error_reply(id, name).as_bytes()).unwrap_err(),
                DecodeError {
                    id: id.cloned(),
                    error: ErrorName::Malformed
                },
                "{name:?}"
            );
        }
        assert!(ErrorName::read(name).is_err(), "{name:?}");
        assert!(ErrorName::parse(name).is_err(), "{name:?}");
        assert!(
            serde_json::from_str::<ErrorName>(&json(name)).is_err(),
            "{name:?}"
        );
    }
}

#[test]
fn the_nine_names_of_the_reply_form_answer_as_before() {
    let names: Vec<&str> = ErrorName::ALL.iter().map(|n| n.as_str()).collect();
    assert_eq!(names, NINE);
    let id = id();
    for error in ErrorName::ALL.iter().copied() {
        let name = error.as_str();
        assert_eq!(ErrorName::parse(name).unwrap(), error);
        assert_eq!(ErrorName::read(name).unwrap(), error);
        assert!(!matches!(error, ErrorName::Declared(_)));
        for id in [Some(&id), None] {
            let bytes = write_reply(id, Err(error)).unwrap();
            assert_eq!(
                String::from_utf8(bytes.clone()).unwrap(),
                error_reply(id, name)
            );
            let reply = decode_reply(&bytes).unwrap();
            assert_eq!(reply.id.as_ref(), id);
            assert_eq!(reply.outcome.unwrap_err(), error);
        }
    }
    let ok = write_reply(Some(&id), Ok(&ReplyResult::Null)).unwrap();
    assert_eq!(
        String::from_utf8(ok).unwrap(),
        format!(r#"{{"v":1,"id":"{ID}","ok":null}}"#)
    );
}

#[test]
fn the_parse_of_the_nine_takes_no_declared_name() {
    for name in declared_error_names().filter(|name| !NINE.contains(name)) {
        assert!(ErrorName::parse(name).is_err(), "{name}");
        assert!(ErrorName::read(name).is_ok(), "{name}");
    }
}

#[test]
fn every_name_that_a_file_of_a_later_run_declares_has_1_to_32_bytes_of_a_to_z_0_to_9_and_underscore(
) {
    assert_eq!(form_faults(declared_error_names()), Vec::<String>::new());
}

#[test]
fn the_check_of_the_form_of_a_declared_name_has_a_case_for_each_way_out_of_the_form() {
    let long = "a".repeat(33);
    let longest = "a".repeat(32);
    assert_eq!(
        form_faults([
            "",
            long.as_str(),
            "Binding",
            "bind-ing",
            "bind ing",
            "bind\u{e9}",
            "binding",
            longest.as_str(),
            "a",
            "z_0_9"
        ]),
        [
            "the error name \"\" has not 1 to 32 bytes of a to z, 0 to 9 and _".to_owned(),
            format!("the error name {long:?} has not 1 to 32 bytes of a to z, 0 to 9 and _"),
            "the error name \"Binding\" has not 1 to 32 bytes of a to z, 0 to 9 and _".to_owned(),
            "the error name \"bind-ing\" has not 1 to 32 bytes of a to z, 0 to 9 and _".to_owned(),
            "the error name \"bind ing\" has not 1 to 32 bytes of a to z, 0 to 9 and _".to_owned(),
            "the error name \"bind\u{e9}\" has not 1 to 32 bytes of a to z, 0 to 9 and _"
                .to_owned(),
        ]
    );
}
