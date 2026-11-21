use super::names::names;

names! {
    /// The closed list of the thirteen causes with which a command is refused (I8.1); `overlap`,
    /// the thirteenth, answers a Switch on whose state the owner refused with `overlap` (ruling
    /// RD-161 R4, WAY A); `words::text`
    /// and every `parse` of this crate answer `malformed`.
    Cause {
        Stale = "stale",
        Presenter = "presenter",
        Early = "early",
        Hold = "hold",
        State = "state",
        Budget = "budget",
        QueueFull = "queue_full",
        Busy = "busy",
        NoService = "no_service",
        UnknownItem = "unknown_item",
        UnknownAgent = "unknown_agent",
        Malformed = "malformed",
        Overlap = "overlap",
    }
}
