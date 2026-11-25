//! The control of the row 61-r6-senders-egress (scripts/r6/ci.d/61-r6-senders-egress.sh). Its one
//! test is ignored. The row's self-test alone runs it, by its exact name with `--ignored`, through
//! the row's own cargo call under the egress check, whose denied line must turn the row red. No other
//! run of the tree passes `--ignored` to it.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Connects to 192.0.2.1:443 and sends nothing. 192.0.2.1 is of TEST-NET-1 (RFC 5737), a range for
/// documentation that no host answers. The test passes whatever the attempt gives: the row reads its
/// red from the egress check's denied line, not from this test.
#[test]
#[ignore = "the control of 61-r6-senders-egress: the row's self-test alone runs it, under the egress check"]
fn the_control_of_the_senders_row_connects_to_a_documentation_address_and_sends_nothing() {
    let address = SocketAddr::from(([192, 0, 2, 1], 443));
    let _ = TcpStream::connect_timeout(&address, Duration::from_secs(2));
}
