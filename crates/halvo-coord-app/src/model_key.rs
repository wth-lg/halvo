//! The person's key for a model's host in the app (r4 U3.18).
#![cfg(any(target_os = "macos", windows))]

/// The host of this build: the origin of the rows of `halvo_route::release::ruling()` without its `https://`, where
/// every row names the same one origin, and none where the ruling holds no row or its rows name two origins. With
/// main's data it is none.
pub fn host() -> Option<String> {
    host_of(
        halvo_route::release::ruling()
            .rows()
            .iter()
            .map(|row| row.origin()),
    )
}

/// The host of `origins`, none where there is none or two of them differ.
fn host_of<'a>(origins: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut origins = origins.into_iter();
    let first = origins.next()?;
    if origins.any(|origin| origin != first) {
        return None;
    }
    first.strip_prefix("https://").map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// U3.18 sentence 1: the host is the one origin of the rows without its `https://`: none for no row, the host for
    /// one row and for two rows of the same origin, none for two origins; main's ruling gives none.
    #[test]
    fn the_host_is_the_one_origin_of_the_rows_without_https() {
        assert_eq!(host_of([]), None);
        assert_eq!(
            host_of(["https://api.host.invalid"]),
            Some("api.host.invalid".to_owned())
        );
        assert_eq!(
            host_of(["https://api.host.invalid", "https://api.host.invalid"]),
            Some("api.host.invalid".to_owned())
        );
        assert_eq!(
            host_of(["https://api.host.invalid", "https://other.host.invalid"]),
            None
        );
        assert_eq!(host(), None);
    }
}
