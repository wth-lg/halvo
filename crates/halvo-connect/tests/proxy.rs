mod common;

use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::fixture::Fixture;
use common::seed::Seed;
use halvo_connect::Mail;

#[test]
fn a_proxy_of_the_environment_gets_no_request() {
    let proxy = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = format!(
        "http://127.0.0.1:{}",
        proxy.local_addr().expect("its address").port()
    );
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if proxy.accept().is_ok() {
            let _ = tx.send(());
        }
    });
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        std::env::set_var(name, &address);
    }
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let fixture = Fixture::start();
    let seed = Seed::load(1);
    let messages = fixture
        .graph_within("mail", Duration::from_secs(3))
        .mail_list(&seed.token(), &seed.rule());
    assert!(
        messages.is_ok_and(|messages| messages.len() == 2),
        "the fixture answers the list"
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "the proxy saw no connection"
    );
}
