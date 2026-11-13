mod common;

use common::*;
use halvo_agent_store::{workbook_path, MemoryStore, PathForm, PathRefused};
use halvo_schema::executable::CoordinatorReply;
use halvo_schema::ErrorName;
use serde_json::{json, Value};

fn slot(name: &str, value: &str) -> Value {
    json!({"name": name, "value": if value.is_empty() { json!([]) } else { json!([value]) }})
}

fn set(store: &mut MemoryStore, a: u32, slots: Vec<Value>) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "set_bindings", "workflow": wf(1), "agent": agent(a), "slots": slots}),
    )
}

fn add(store: &mut MemoryStore, a: u32, origin: &str) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "add_origin", "workflow": wf(1), "agent": agent(a), "run": run(1), "origin": origin, "way": "sign_in"}),
    )
}

fn newest(store: &MemoryStore) -> (u32, Vec<String>, Vec<(String, String)>) {
    let b = bindings(store)
        .into_iter()
        .max_by_key(|b| b.rev)
        .expect("a revision of the bindings");
    (
        b.rev.get(),
        b.origins.iter().map(|o| o.as_str().to_owned()).collect(),
        b.slots
            .iter()
            .map(|s| (s.name.as_str().to_owned(), s.value.joined()))
            .collect(),
    )
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

fn native(rest: &str) -> String {
    if cfg!(windows) {
        format!("C:\\{}", rest.replace('/', "\\"))
    } else {
        format!("/{rest}")
    }
}

#[test]
fn set_bindings_merges_by_name_into_the_next_revision() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        set(
            &mut store,
            1,
            vec![slot("payee", "Northwind"), slot("instrument", "card")]
        ),
        Ok(rev(1))
    );
    assert_eq!(
        newest(&store),
        (
            1,
            vec![],
            pairs(&[("payee", "Northwind"), ("instrument", "card")])
        )
    );
    assert_eq!(
        set(
            &mut store,
            1,
            vec![
                slot("start", "https://pay.test:443/"),
                slot("payee", "Contoso")
            ]
        ),
        Ok(rev(2))
    );
    assert_eq!(
        newest(&store),
        (
            2,
            vec![],
            pairs(&[
                ("payee", "Contoso"),
                ("instrument", "card"),
                ("start", "https://pay.test:443/")
            ])
        )
    );
    assert_eq!(set(&mut store, 1, vec![]), Ok(rev(3)));
    assert_eq!(newest(&store).2.len(), 3);
    assert_eq!(bindings(&store).len(), 3);
}

#[test]
fn set_bindings_keeps_a_slot_that_it_does_not_name_and_the_origins() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        set(&mut store, 1, vec![slot("payee", "Northwind")]),
        Ok(rev(1))
    );
    assert_eq!(add(&mut store, 1, "https://pay.test:443"), Ok(rev(2)));
    assert_eq!(
        set(&mut store, 1, vec![slot("instrument", "card")]),
        Ok(rev(3))
    );
    assert_eq!(
        newest(&store),
        (
            3,
            vec!["https://pay.test:443".to_owned()],
            pairs(&[("payee", "Northwind"), ("instrument", "card")])
        )
    );
}

#[test]
fn add_origin_adds_one_origin_and_keeps_the_slots() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(add(&mut store, 1, "https://pay.test:443"), Ok(rev(1)));
    assert_eq!(
        newest(&store),
        (1, vec!["https://pay.test:443".to_owned()], vec![])
    );
    assert_eq!(
        set(&mut store, 1, vec![slot("payee", "Northwind")]),
        Ok(rev(2))
    );
    assert_eq!(add(&mut store, 1, "https://login.test:443"), Ok(rev(3)));
    assert_eq!(
        newest(&store),
        (
            3,
            vec![
                "https://pay.test:443".to_owned(),
                "https://login.test:443".to_owned()
            ],
            pairs(&[("payee", "Northwind")])
        )
    );
}

#[test]
fn add_origin_of_an_origin_already_there_stores_nothing_and_answers_the_newest() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(add(&mut store, 1, "https://pay.test:443"), Ok(rev(1)));
    assert_eq!(add(&mut store, 1, "https://pay.test:443"), Ok(rev(1)));
    assert_eq!(bindings(&store).len(), 1);
    assert_eq!(
        set(&mut store, 1, vec![slot("payee", "Northwind")]),
        Ok(rev(2))
    );
    assert_eq!(add(&mut store, 1, "https://pay.test:443"), Ok(rev(2)));
    assert_eq!(bindings(&store).len(), 2);
}

#[test]
fn the_bindings_of_one_agent_are_its_own() {
    let mut store = store_with("utility_bill_payment");
    store.put_workflow(workflow(&wf(2), "email", "approved", None));
    create(&mut store, 2, 2).expect("create_agent");
    assert_eq!(
        set(&mut store, 1, vec![slot("payee", "Northwind")]),
        Ok(rev(1))
    );
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "set_bindings", "workflow": wf(2), "agent": agent(2), "slots": [slot("to", "a@b.test")]})
        ),
        Ok(rev(1))
    );
    assert_eq!(
        set(&mut store, 1, vec![slot("instrument", "card")]),
        Ok(rev(2))
    );
    assert_eq!(
        newest(&store),
        (
            2,
            vec![],
            pairs(&[("payee", "Northwind"), ("instrument", "card")])
        )
    );
}

#[test]
fn bindings_refusals_store_nothing() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        set(&mut store, 2, vec![slot("payee", "Northwind")]),
        Err(declared("binding"))
    );
    assert_eq!(
        add(&mut store, 2, "https://pay.test:443"),
        Err(declared("binding"))
    );
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "set_bindings", "workflow": wf(9), "agent": agent(1), "slots": []})
        ),
        Err(ErrorName::NoWorkflow)
    );
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "add_origin", "workflow": wf(9), "agent": agent(1), "run": run(1), "origin": "https://pay.test:443", "way": "new_origin"})
        ),
        Err(ErrorName::NoWorkflow)
    );
    assert!(bindings(&store).is_empty());
}

#[test]
fn set_bindings_refuses_a_workbook_that_is_no_absolute_path_in_one_spelling() {
    let mut store = store_with("spreadsheet_fill");
    for refused in [
        "Book.xlsx".to_owned(),
        "dir/Book.xlsx".to_owned(),
        String::new(),
        native("a/./Book.xlsx"),
        native("a/../Book.xlsx"),
        native("../Book.xlsx"),
        native("a//Book.xlsx"),
        native("a/Book.xlsx/"),
    ] {
        assert_eq!(
            set(
                &mut store,
                1,
                vec![slot("range", "A1:B2"), slot("workbook", &refused)]
            ),
            Err(declared("binding")),
            "{refused}"
        );
    }
    assert!(bindings(&store).is_empty());
    let book = native("Users/p/Books/Book.xlsx");
    assert_eq!(
        set(
            &mut store,
            1,
            vec![slot("workbook", &book), slot("range", "Book.xlsx")]
        ),
        Ok(rev(1))
    );
    assert_eq!(
        newest(&store).2,
        pairs(&[("workbook", &book), ("range", "Book.xlsx")])
    );
    assert_eq!(
        set(&mut store, 1, vec![slot("start", "a//b/../c")]),
        Ok(rev(2))
    );
}

#[test]
fn the_posix_form() {
    let ok = [
        "/Users/p/Book.xlsx",
        "/Users/p/.Book.xlsx",
        "/a..b/c.",
        "/a\\b",
        "/a",
    ];
    for path in ok {
        assert_eq!(workbook_path(path, PathForm::Posix), Ok(()), "{path}");
    }
    let refused = [
        ("Book.xlsx", PathRefused::Relative),
        ("", PathRefused::Relative),
        ("./Book.xlsx", PathRefused::Relative),
        ("C:\\Users\\Book.xlsx", PathRefused::Relative),
        ("/", PathRefused::EmptyStep),
        ("//a", PathRefused::EmptyStep),
        ("/a//b", PathRefused::EmptyStep),
        ("/a/b/", PathRefused::EmptyStep),
        ("/a/./b", PathRefused::DotStep),
        ("/a/../b", PathRefused::DotStep),
        ("/..", PathRefused::DotStep),
        ("/a/.", PathRefused::DotStep),
    ];
    for (path, cause) in refused {
        assert_eq!(workbook_path(path, PathForm::Posix), Err(cause), "{path}");
    }
}

#[test]
fn the_windows_form() {
    let ok = [
        "C:\\Users\\p\\Book.xlsx",
        "c:/Users/p/Book.xlsx",
        "Z:\\a",
        "C:\\a..b\\.c",
    ];
    for path in ok {
        assert_eq!(workbook_path(path, PathForm::Windows), Ok(()), "{path}");
    }
    let refused = [
        ("Book.xlsx", PathRefused::Relative),
        ("", PathRefused::Relative),
        ("\\\\server\\share\\Book.xlsx", PathRefused::Relative),
        ("\\Users\\Book.xlsx", PathRefused::Relative),
        ("/Users/p/Book.xlsx", PathRefused::Relative),
        ("C:Users\\Book.xlsx", PathRefused::Relative),
        ("1:\\Book.xlsx", PathRefused::Relative),
        ("CC:\\Book.xlsx", PathRefused::Relative),
        ("C:", PathRefused::Relative),
        ("C:\\", PathRefused::EmptyStep),
        ("C:\\a\\\\b", PathRefused::EmptyStep),
        ("C:\\a\\/b", PathRefused::EmptyStep),
        ("C:/a//b", PathRefused::EmptyStep),
        ("C:\\a\\", PathRefused::EmptyStep),
        ("C:\\a\\.\\b", PathRefused::DotStep),
        ("C:\\a\\..\\b", PathRefused::DotStep),
        ("C:/a/../b", PathRefused::DotStep),
    ];
    for (path, cause) in refused {
        assert_eq!(workbook_path(path, PathForm::Windows), Err(cause), "{path}");
    }
}

#[test]
fn the_native_form_is_the_system_s() {
    let expected = if cfg!(windows) {
        PathForm::Windows
    } else {
        PathForm::Posix
    };
    assert_eq!(PathForm::NATIVE, expected);
}
