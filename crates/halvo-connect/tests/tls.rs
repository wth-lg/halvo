mod common;

use std::io::Read;
use std::net::TcpListener;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use common::test_only_untrusted_tls_key::{
    TEST_ONLY_UNTRUSTED_CERT_DER, TEST_ONLY_UNTRUSTED_KEY_PKCS8,
};
use halvo_connect::{CallError, Effect, Graph, Mail, ReplyRule, Route, Token};
use halvo_schema::executable::WebOrigin;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{AlertDescription, ServerConfig, ServerConnection, SupportedProtocolVersion};

struct Outcome {
    error: Option<rustls::Error>,
    application_bytes: usize,
}

fn serve(version: &'static SupportedProtocolVersion) -> (u16, mpsc::Receiver<Outcome>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[version])
        .expect("the version")
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(TEST_ONLY_UNTRUSTED_CERT_DER.to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                TEST_ONLY_UNTRUSTED_KEY_PKCS8.to_vec(),
            )),
        )
        .expect("the test-only certificate and key");
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("its address").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut tcp, _)) = listener.accept() else {
            return;
        };
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
        let mut conn = ServerConnection::new(Arc::new(config)).expect("a server connection");
        let mut outcome = Outcome {
            error: None,
            application_bytes: 0,
        };
        loop {
            while conn.wants_write() {
                if conn.write_tls(&mut tcp).is_err() {
                    break;
                }
            }
            match conn.read_tls(&mut tcp) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            match conn.process_new_packets() {
                Ok(state) => {
                    let n = state.plaintext_bytes_to_read();
                    if n > 0 {
                        let mut buf = vec![0u8; n];
                        let _ = conn.reader().read(&mut buf);
                        outcome.application_bytes += n;
                    }
                }
                Err(error) => {
                    outcome.error = Some(error);
                    break;
                }
            }
        }
        let _ = tx.send(outcome);
    });
    (port, rx)
}

fn certificate_alert(alert: AlertDescription) -> bool {
    matches!(
        alert,
        AlertDescription::BadCertificate
            | AlertDescription::UnsupportedCertificate
            | AlertDescription::CertificateRevoked
            | AlertDescription::CertificateExpired
            | AlertDescription::CertificateUnknown
            | AlertDescription::UnknownCA
            | AlertDescription::AccessDenied
    )
}

#[test]
fn a_certificate_that_the_system_does_not_trust_ends_the_call_before_any_request() {
    let limit = Duration::from_secs(5);
    let versions: [(&str, &'static SupportedProtocolVersion); 2] = [
        ("TLS 1.2", &rustls::version::TLS12),
        ("TLS 1.3", &rustls::version::TLS13),
    ];
    for (name, version) in versions {
        let (port, outcome) = serve(version);
        let origin =
            WebOrigin::parse(&format!("https://mail.agent.test:{port}")).expect("an origin");
        let graph = Graph::new(&origin, Route::Loopback, limit).expect("the client builds");
        let token = Token::new("stub-token-1".to_owned()).expect("a token");
        let rule = ReplyRule::new(
            "me@home.test".to_owned(),
            vec!["ann@people.test".to_owned()],
        );
        let began = Instant::now();
        let error = graph
            .mail_list(&token, &rule)
            .expect_err("the certificate is refused");
        assert_eq!(error, CallError::NotSent, "{name}");
        assert_eq!(error.effect(), Effect::NotMade, "{name}");
        assert!(began.elapsed() < limit, "{name}: within the limit");
        let outcome = outcome
            .recv_timeout(Duration::from_secs(15))
            .expect("the stand-in ends");
        assert_eq!(
            outcome.application_bytes, 0,
            "{name}: the stand-in received application data"
        );
        assert!(
            matches!(outcome.error, Some(rustls::Error::AlertReceived(alert)) if certificate_alert(alert)),
            "{name}: the client did not refuse the certificate after it came: {:?}",
            outcome.error
        );
    }
}
