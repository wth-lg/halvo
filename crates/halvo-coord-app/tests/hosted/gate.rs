/// Whether the dialog cases may run: on a hosted runner of GitHub Actions alone, which sets
/// `GITHUB_ACTIONS` to `true` and `RUNNER_ENVIRONMENT` to `github-hosted`. Every other pair,
/// a missing variable among them, is any other machine, where no dialog is shown.
pub fn hosted(github_actions: Option<&str>, runner_environment: Option<&str>) -> bool {
    github_actions == Some("true") && runner_environment == Some("github-hosted")
}

/// What the target does at its start.
#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    /// The cases run.
    Run,
    /// The target prints its one line and ends with the status 0.
    Off,
    /// The target prints this line and fails.
    Fail(String),
}

/// The decision at the early return, from a read of the two variables made at that return,
/// apart from the read that chose the branch: `Off` when it says not hosted, and `Fail`, with
/// a line that names what it read, when it says hosted.
pub fn at_return(github_actions: Option<&str>, runner_environment: Option<&str>) -> Start {
    if hosted(github_actions, runner_environment) {
        Start::Fail(format!(
            "FAIL: the first read said that this is no hosted runner and the read at the return says that it is: GITHUB_ACTIONS={github_actions:?} RUNNER_ENVIRONMENT={runner_environment:?}"
        ))
    } else {
        Start::Off
    }
}

/// `hosted` of the two variables that `read` gives.
pub fn hosted_by(read: &dyn Fn(&str) -> Option<String>) -> bool {
    let actions = read("GITHUB_ACTIONS");
    let environment = read("RUNNER_ENVIRONMENT");
    hosted(actions.as_deref(), environment.as_deref())
}

/// The target's start over `read`, a reader of the process's variables: `Run` when a first
/// read says hosted, and else the decision of `at_return` over a second read.
pub fn start(read: &dyn Fn(&str) -> Option<String>) -> Start {
    if hosted_by(read) {
        return Start::Run;
    }
    let actions = read("GITHUB_ACTIONS");
    let environment = read("RUNNER_ENVIRONMENT");
    at_return(actions.as_deref(), environment.as_deref())
}

/// A variable of this process.
pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
