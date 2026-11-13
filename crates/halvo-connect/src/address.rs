/// Whether `a` and `b` are one address of the reply rule: the same bytes without regard to ASCII case. A byte
/// outside ASCII compares as it is. `ReplyRule::selects` holds every address against another with this function,
/// and the runner tests a message again with it (A5.1, A11.2):
///
/// ```
/// assert!(halvo_connect::same_address("ann@people.test", "ANN@People.TEST"));
/// assert!(!halvo_connect::same_address("élan@people.test", "ÉLAN@people.test"));
/// ```
pub fn same_address(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
