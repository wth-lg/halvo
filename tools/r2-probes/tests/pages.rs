use r2_probes::pages::{self, Server};
use r2_probes::sys;
use std::io::{Read, Write};
use std::net::TcpStream;

#[test]
fn every_text_page_has_a_title_and_a_known_text() {
    for page in pages::TEXT_PAGES {
        assert!(pages::title(page).starts_with("Halvo probe text page"));
        let known = pages::known_text(page).unwrap();
        assert!(known.chars().count() > 1000, "{page} is short");
        assert!(!known.contains('<') && !known.contains('>'));
    }
}

#[test]
fn the_known_text_is_the_text_of_the_body_in_document_order() {
    let known = pages::known_text("text-1").unwrap();
    assert!(known
        .starts_with("Brightwater Utilities account summary Statement for the period 1 October"));
    assert!(known.ends_with("with the account number in the subject line."));
    assert!(!known.contains("font"));
}

#[test]
fn both_documents_have_a_known_text() {
    assert!(pages::known_text("writer")
        .unwrap()
        .starts_with("Notice of rent review for 18 Harlow Street, Flat 3 Eastmere Lettings"));
    assert!(pages::known_text("calc")
        .unwrap()
        .starts_with("Item Quantity Unit price Total Extractor fan 1 84.5 84.5"));
}

#[test]
fn the_pages_of_the_private_window_cell_carry_no_marker() {
    for page in ["page-one", "page-two"] {
        assert!(!sys::marked(&pages::title(page)));
        assert!(!sys::marked(&pages::known_text(page).unwrap()));
    }
    assert!(sys::marked("New Tab - Google Chrome (Incognito)"));
}

#[test]
fn the_server_serves_a_page_and_remembers_its_token() {
    let server = Server::start().unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .write_all(b"GET /text-1.html?r=t9 HTTP/1.1\r\nHost: probe\r\n\r\n")
        .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 OK"));
    assert!(answer.contains("Brightwater Utilities"));
    assert!(server.requested("t9"));
    assert!(!server.requested("t1"));
    assert_eq!(
        server.url("text-1", "t9"),
        format!("http://127.0.0.1:{}/text-1.html?r=t9", server.port)
    );
}
