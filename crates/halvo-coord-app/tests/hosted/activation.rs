use std::time::Duration;

/// How the hosted target's wait for its app to be active ended (ruling RD-107).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waited {
    /// The app read active after `asks` asks.
    Active { asks: u32 },
    /// The app never read active within the limit, after `asks` asks.
    Inactive { asks: u32 },
}

/// Asks once for the app to be active and reads whether it is; while it is not, passes `every`
/// through `pass` and asks and reads again, until `limit` has passed in all.
pub fn wait(
    mut ask: impl FnMut(),
    mut active: impl FnMut() -> bool,
    mut pass: impl FnMut(Duration),
    every: Duration,
    limit: Duration,
) -> Waited {
    let mut asks = 1;
    let mut passed = Duration::ZERO;
    ask();
    while !active() {
        if passed + every > limit {
            return Waited::Inactive { asks };
        }
        pass(every);
        passed += every;
        asks += 1;
        ask();
    }
    Waited::Active { asks }
}
