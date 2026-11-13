/// Whether the dialog cases may run: on a hosted runner of GitHub Actions alone, which sets
/// `GITHUB_ACTIONS` to `true` and `RUNNER_ENVIRONMENT` to `github-hosted`. Every other pair,
/// a missing variable among them, is any other machine, where no dialog is shown.
pub fn hosted(github_actions: Option<&str>, runner_environment: Option<&str>) -> bool {
    github_actions == Some("true") && runner_environment == Some("github-hosted")
}

/// `hosted` of the two variables of this process.
pub fn hosted_here() -> bool {
    let actions = std::env::var("GITHUB_ACTIONS").ok();
    let environment = std::env::var("RUNNER_ENVIRONMENT").ok();
    hosted(actions.as_deref(), environment.as_deref())
}
