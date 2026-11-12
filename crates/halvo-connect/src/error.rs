use std::fmt;

/// Why a call gave no result. No variant holds text of a request, of an answer or of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The request did not leave: no connection or no TLS session to the origin could be made.
    NotSent,
    /// The provider answered 401: the token is not valid for it.
    Unauthorized,
    /// The provider answered this status, which is neither 401 nor the success status that Microsoft Graph
    /// documents for the request and the fixture answers: 200 for a GET and for the PATCH, 201 for createReply,
    /// 202 for send, 204 for DELETE. Another 2xx is such a status too. No redirect is followed.
    Status(u16),
    /// The request left and its answer did not arrive whole within the time limit of the adapter.
    NoAnswer,
    /// The answer arrived and does not have the form of the face.
    Answer(AnswerFault),
}

/// What is wrong with an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerFault {
    /// The body is not JSON of the form that the operation reads.
    Form,
    /// The list goes on in another page (`@odata.nextLink`), which no operation requests.
    Paged,
    /// Two items of the list have the same id.
    Repeated,
    /// An id is empty, `.` or `..`.
    Id,
}

/// Whether the effect of a call of the class `send` or `delete` that returned an error was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The provider made nothing: the request did not leave, or the provider answered a status from 400 to 499.
    NotMade,
    /// Nothing says whether the provider made it: the request left and no whole answer came, or the provider
    /// answered a status that is neither the request's success status nor in 400 to 499.
    Unknown,
}

/// Why `Graph::new` gave no adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// The origin is `http` and the route is `System`: the token would cross a network in clear.
    Cleartext,
    /// The route is `Loopback` and the URL parser reads the origin's host as an IP address, in any of its written
    /// forms, which no mapping of a host name redirects: the token would go to that address.
    Address,
    /// The TLS configuration or the HTTP client could not be built on this system.
    Client,
}

impl CallError {
    /// What the error says of the effect of a call of the class `send` or `delete` (A4.3, A11.2).
    pub fn effect(&self) -> Effect {
        match self {
            CallError::NotSent | CallError::Unauthorized => Effect::NotMade,
            CallError::Status(status) if (400..500).contains(status) => Effect::NotMade,
            CallError::Status(_) | CallError::NoAnswer | CallError::Answer(_) => Effect::Unknown,
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::NotSent => f.write_str("the request did not leave"),
            CallError::Unauthorized => f.write_str("the provider answered 401"),
            CallError::Status(status) => write!(f, "the provider answered the status {status}"),
            CallError::NoAnswer => f.write_str("no whole answer came within the time limit"),
            CallError::Answer(fault) => f.write_str(match fault {
                AnswerFault::Form => "the answer is not of the form of the face",
                AnswerFault::Paged => "the answer goes on in another page",
                AnswerFault::Repeated => "the answer holds one id twice",
                AnswerFault::Id => "the answer holds an id that is empty, . or ..",
            }),
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BuildError::Cleartext => "an http origin needs the loopback route",
            BuildError::Address => {
                "the loopback route maps a host name, and the host of this origin is an address"
            }
            BuildError::Client => "the TLS configuration or the HTTP client could not be built",
        })
    }
}

impl std::error::Error for CallError {}

impl std::error::Error for BuildError {}

#[cfg(test)]
mod tests;
