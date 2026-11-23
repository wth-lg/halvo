//! The door's stand-in form, compiled only under the cargo feature `loopback`, which tests and the
//! trial's stand-in form turn on (the coordinator's log line 16582). Its ruling is the router's
//! public ruling for tests, whose one origin is made from the port number alone, so a build that
//! carried the feature by a fault could still reach nothing beyond the loopback. This file is the
//! one file of the door that names that maker.

use halvo_connect::model::Adapter;
use halvo_route::ruling::Ruling;

/// The model word of the rows of the stand-in ruling.
pub const MODEL: &str = "stand-in-model";

/// The argument that gives the port of the stand-in host; a plain build has no such argument.
pub const PORT_ARGUMENT: &str = "--loopback-port";

/// The ruling of the router for a stand-in host on 127.0.0.1 at `port`: one row for each hosted
/// tier, each with the origin `http://127.0.0.1:<port>`, the model word `MODEL` and the door's one
/// adapter word.
pub fn ruling(port: u16) -> Ruling {
    Ruling::loopback_for_tests(port, MODEL, Adapter::AnthropicMessages.word())
}
