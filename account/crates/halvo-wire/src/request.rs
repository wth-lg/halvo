use std::fmt;

use crate::{
    types::{Cap, CapBody},
    CAP_AMOUNT_MAX, CAP_AMOUNT_MIN,
};

/// One app route of `account/schema/routes.v1.json`: what the builder sends and what it reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub request: Option<&'static str>,
    pub answer: &'static str,
    pub limit: usize,
}

/// The six routes of kind `app` of `account/schema/routes.v1.json`, in its order; a test fails when they differ.
pub const APP_ROUTES: [Route; 6] = [
    Route {
        name: "checkout-start",
        method: "POST",
        path: "/v1/checkout",
        request: Some("CapBody"),
        answer: "CheckoutAnswer",
        limit: 45,
    },
    Route {
        name: "status",
        method: "GET",
        path: "/v1/subscription",
        request: None,
        answer: "StatusAnswer",
        limit: 0,
    },
    Route {
        name: "cap-get",
        method: "GET",
        path: "/v1/cap",
        request: None,
        answer: "CapGetAnswer",
        limit: 0,
    },
    Route {
        name: "cap-set",
        method: "PUT",
        path: "/v1/cap",
        request: Some("CapBody"),
        answer: "CapSetAnswer",
        limit: 45,
    },
    Route {
        name: "manage",
        method: "POST",
        path: "/v1/manage",
        request: None,
        answer: "ManageAnswer",
        limit: 0,
    },
    Route {
        name: "account-delete",
        method: "DELETE",
        path: "/v1/account",
        request: None,
        answer: "DeleteAnswer",
        limit: 0,
    },
];

/// The three header names of `account/schema/routes.v1.json`; the builder sends no other.
pub const HEADER_NAMES: [&str; 3] = ["halvo-account", "authorization", "content-type"];

const CONTENT_TYPE: &str = "application/json";

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

/// An account id: a version 4 UUID in lower case, 36 characters, the value of the header `halvo-account`.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountId(String);

impl AccountId {
    /// Reads a text that the header pattern of `halvo-account` matches, and nothing else.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        let fits = bytes.len() == 36
            && bytes.iter().enumerate().all(|(at, byte)| match at {
                8 | 13 | 18 | 23 => *byte == b'-',
                14 => *byte == b'4',
                19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
                _ => is_lower_hex(*byte),
            });
        fits.then(|| Self(text.to_string()))
    }

    /// Makes the id of 16 random bytes: it sets the bits of version 4 and of the variant.
    pub fn from_random(mut bytes: [u8; 16]) -> Self {
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex = lower_hex(&bytes);
        Self(format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        ))
    }

    /// The id as the header carries it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountId(..)")
    }
}

/// A credential: 64 lower-case hexadecimal characters, 32 random bytes; `authorization` carries it after `Bearer `.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    /// Reads 64 lower-case hexadecimal characters, and nothing else.
    pub fn parse(text: &str) -> Option<Self> {
        let fits = text.len() == 64 && text.bytes().all(is_lower_hex);
        fits.then(|| Self(text.to_string()))
    }

    /// Makes the credential of 32 random bytes.
    pub fn from_random(bytes: [u8; 32]) -> Self {
        Self(lower_hex(&bytes))
    }

    /// The 64 characters of the credential.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(..)")
    }
}

/// A call of one app route, with the cap that the two routes with a body carry.
#[derive(Clone, Debug)]
pub enum Call {
    Status,
    CheckoutStart(Cap),
    CapGet,
    CapSet(Cap),
    Manage,
    AccountDelete,
}

impl Call {
    /// The route that the call sends to.
    pub fn route(&self) -> &'static Route {
        let name = match self {
            Self::CheckoutStart(_) => "checkout-start",
            Self::Status => "status",
            Self::CapGet => "cap-get",
            Self::CapSet(_) => "cap-set",
            Self::Manage => "manage",
            Self::AccountDelete => "account-delete",
        };
        APP_ROUTES
            .iter()
            .find(|route| route.name == name)
            .expect("APP_ROUTES holds every route that a call names")
    }
}

/// A request to send to the backend's origin: the method, the path, the headers in order and the body.
/// [`build`] is its only constructor, and its fields are private, so a caller reads a request and
/// cannot make or change one.
///
/// ```
/// use halvo_wire::request::{build, AccountId, Call, Credential};
/// let request = build(&Call::Status, &AccountId::from_random([1; 16]), &Credential::from_random([2; 32]))
///     .expect("a call without a cap is built");
/// assert_eq!((request.method(), request.path(), request.body()), ("GET", "/v1/subscription", None));
/// ```
///
/// ```compile_fail,E0451
/// let request = halvo_wire::request::Request { method: "GET", path: "/x", headers: Vec::new(), body: None };
/// ```
///
/// ```compile_fail,E0616
/// fn change(request: &mut halvo_wire::request::Request) {
///     request.path = "/v1/done";
/// }
/// ```
///
/// ```compile_fail,E0616
/// fn change(request: &mut halvo_wire::request::Request) {
///     request.method = "PATCH";
/// }
/// ```
///
/// ```compile_fail,E0616
/// fn change(request: &mut halvo_wire::request::Request) {
///     request.headers = vec![("x-other", String::new())];
/// }
/// ```
///
/// ```compile_fail,E0616
/// fn change(request: &mut halvo_wire::request::Request) {
///     request.body = Some(b"{}".to_vec());
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    method: &'static str,
    path: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Option<Vec<u8>>,
}

impl Request {
    /// The method of the call's route.
    pub fn method(&self) -> &'static str {
        self.method
    }

    /// The path of the call's route.
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// The headers, in the order in which they are sent.
    pub fn headers(&self) -> &[(&'static str, String)] {
        &self.headers
    }

    /// The body: a `CapBody` as compact JSON for a route with a request schema, and none otherwise.
    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| *name).collect();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("header_names", &names)
            .field("body_bytes", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// Why the builder made no request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The cap amount lies outside `CAP_AMOUNT_MIN` to `CAP_AMOUNT_MAX`.
    AmountOutOfRange(u64),
}

/// Builds the request of `call`: its route's method and path, the headers `halvo-account` and
/// `authorization`, and for a route with a request schema a `CapBody` as compact JSON with
/// `content-type: application/json`. It refuses a cap amount outside the schema's bounds.
pub fn build(
    call: &Call,
    account: &AccountId,
    credential: &Credential,
) -> Result<Request, Refused> {
    let route = call.route();
    let mut headers = vec![
        (HEADER_NAMES[0], account.as_str().to_string()),
        (HEADER_NAMES[1], format!("Bearer {}", credential.as_str())),
    ];
    let body = match call {
        Call::CheckoutStart(cap) | Call::CapSet(cap) => {
            let amount = cap.amount.get();
            if !(CAP_AMOUNT_MIN..=CAP_AMOUNT_MAX).contains(&amount) {
                return Err(Refused::AmountOutOfRange(amount));
            }
            headers.push((HEADER_NAMES[2], CONTENT_TYPE.to_string()));
            let body = CapBody { cap: cap.clone() };
            Some(serde_json::to_vec(&body).expect("a CapBody always serializes"))
        }
        Call::Status | Call::CapGet | Call::Manage | Call::AccountDelete => None,
    };
    Ok(Request {
        method: route.method,
        path: route.path,
        headers,
        body,
    })
}
