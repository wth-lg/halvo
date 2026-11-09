//! The Windows background service: a per-user logon Scheduled Task.
//!
//! A capture backend needs the interactive desktop (Windows.Graphics.Capture,
//! Raw Input and WinEvent hooks all run in the user's session), so the right
//! model is a per-user Scheduled Task triggered `ONLOGON` with an interactive
//! token, not a session-0 Windows Service (researcher-1, researcher-4). The task
//! is created through `schtasks.exe`; the argument vectors it is driven with are
//! built by the pure functions here, which is what the unit tests pin.
//!
//! Install is idempotent: it queries first and rewrites the task only when it is
//! absent or registered against a different command, and the create uses `/F`, so
//! any number of runs converge to one task with one definition.

use std::process::Command;

use super::{ServiceError, ServiceInstaller, ServiceStatus};

/// The Scheduled Task name, under a `Halvo` folder so it is easy to find and
/// remove and never collides with another task.
pub const DEFAULT_TASK_NAME: &str = "Halvo\\HalvoCapture";

/// A Scheduled Task definition: its name and the command it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    /// The `/TN` task name, e.g. `Halvo\HalvoCapture`.
    pub task_name: String,
    /// The absolute path to the capture executable.
    pub program: String,
    /// Arguments passed to the executable at logon.
    pub args: Vec<String>,
}

impl TaskSpec {
    /// A spec for the capture daemon at `program`, under the default task name,
    /// run with the daemon flag so the logon task starts the always-on capture.
    pub fn for_capture_daemon(program: impl Into<String>) -> Self {
        Self {
            task_name: DEFAULT_TASK_NAME.to_string(),
            program: program.into(),
            args: vec!["--service-run".to_string()],
        }
    }

    /// The `/TR` value: the quoted program followed by its arguments. The program
    /// is wrapped in literal quotes so a path with spaces stays one token to the
    /// Task Scheduler.
    pub fn task_run(&self) -> String {
        let mut run = format!("\"{}\"", self.program);
        for arg in &self.args {
            run.push(' ');
            run.push_str(arg);
        }
        run
    }
}

/// Build the `schtasks /Create` arguments for `spec`: a logon-triggered task that
/// runs with the user's limited privilege and an interactive token (so it can
/// reach the desktop), replacing any existing task of the same name (`/F`).
pub fn build_create_args(spec: &TaskSpec) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/F".to_string(),
        "/SC".to_string(),
        "ONLOGON".to_string(),
        "/RL".to_string(),
        "LIMITED".to_string(),
        "/IT".to_string(),
        "/TN".to_string(),
        spec.task_name.clone(),
        "/TR".to_string(),
        spec.task_run(),
    ]
}

/// Build the `schtasks /Query` arguments that test whether the task exists.
pub fn build_query_args(task_name: &str) -> Vec<String> {
    vec![
        "/Query".to_string(),
        "/TN".to_string(),
        task_name.to_string(),
    ]
}

/// Build the `schtasks /Query /XML ONE` arguments that dump the task definition,
/// so an install can tell whether the existing task already runs our command.
pub fn build_query_xml_args(task_name: &str) -> Vec<String> {
    vec![
        "/Query".to_string(),
        "/TN".to_string(),
        task_name.to_string(),
        "/XML".to_string(),
        "ONE".to_string(),
    ]
}

/// Build the `schtasks /Delete` arguments that remove the task without a prompt.
pub fn build_delete_args(task_name: &str) -> Vec<String> {
    vec![
        "/Delete".to_string(),
        "/F".to_string(),
        "/TN".to_string(),
        task_name.to_string(),
    ]
}

/// Whether an existing task's XML already registers `program` as its command, so
/// install can skip the rewrite. The match is case-insensitive because Windows
/// paths are, and ignores the surrounding `<Command>` quoting.
pub fn task_registers(existing_xml: &str, program: &str) -> bool {
    let haystack = existing_xml.to_ascii_lowercase();
    let needle = program.to_ascii_lowercase();
    !needle.is_empty() && haystack.contains(&needle)
}

/// Installs, removes and reports the capture daemon's per-user logon task by
/// driving `schtasks.exe` with the argument builders above.
pub struct WindowsTaskInstaller {
    spec: TaskSpec,
}

impl WindowsTaskInstaller {
    pub fn new(spec: TaskSpec) -> Self {
        Self { spec }
    }

    fn run_schtasks(args: &[String]) -> Result<std::process::Output, ServiceError> {
        Command::new("schtasks.exe")
            .args(args)
            .output()
            .map_err(|e| ServiceError::Op(format!("schtasks.exe failed to launch: {e}")))
    }

    /// The task's current definition XML, or `None` if the task does not exist.
    fn existing_definition(&self) -> Result<Option<String>, ServiceError> {
        let out = Self::run_schtasks(&build_query_xml_args(&self.spec.task_name))?;
        if out.status.success() {
            Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
        } else {
            Ok(None)
        }
    }
}

impl ServiceInstaller for WindowsTaskInstaller {
    fn install(&self) -> Result<(), ServiceError> {
        // Idempotent: if the task already runs our command, leave it; otherwise
        // (re)create it. The create carries `/F`, so an out-of-date task is
        // overwritten rather than duplicated.
        if let Some(xml) = self.existing_definition()? {
            if task_registers(&xml, &self.spec.program) {
                return Ok(());
            }
        }
        let out = Self::run_schtasks(&build_create_args(&self.spec))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(ServiceError::Op(format!(
                "schtasks /Create failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn uninstall(&self) -> Result<(), ServiceError> {
        let out = Self::run_schtasks(&build_delete_args(&self.spec.task_name))?;
        // A missing task is already the desired end state, so deleting one that is
        // not there is success, which keeps uninstall idempotent.
        if out.status.success() || self.status()? == ServiceStatus::NotInstalled {
            Ok(())
        } else {
            Err(ServiceError::Op(format!(
                "schtasks /Delete failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn status(&self) -> Result<ServiceStatus, ServiceError> {
        let out = Self::run_schtasks(&build_query_args(&self.spec.task_name))?;
        if out.status.success() {
            Ok(ServiceStatus::Installed)
        } else {
            Ok(ServiceStatus::NotInstalled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TaskSpec {
        TaskSpec::for_capture_daemon("C:\\Program Files\\Halvo\\halvo-capture.exe")
    }

    #[test]
    fn create_args_are_a_logon_task_with_an_interactive_token() {
        let args = build_create_args(&spec());
        // The ordered flags the Task Scheduler needs for a per-user logon task.
        for expected in [
            "/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/IT", "/TN", "/TR",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "missing {expected} in {args:?}"
            );
        }
        let tn = args.iter().position(|a| a == "/TN").unwrap();
        assert_eq!(args[tn + 1], DEFAULT_TASK_NAME);
    }

    #[test]
    fn task_run_quotes_the_program_and_keeps_args() {
        let run = spec().task_run();
        assert_eq!(
            run,
            "\"C:\\Program Files\\Halvo\\halvo-capture.exe\" --service-run"
        );
    }

    #[test]
    fn query_and_delete_target_the_named_task() {
        assert_eq!(
            build_query_args(DEFAULT_TASK_NAME),
            vec!["/Query", "/TN", DEFAULT_TASK_NAME]
        );
        assert_eq!(
            build_delete_args(DEFAULT_TASK_NAME),
            vec!["/Delete", "/F", "/TN", DEFAULT_TASK_NAME]
        );
    }

    #[test]
    fn task_registers_matches_case_insensitively_and_rejects_other_commands() {
        let xml = "<Command>\"C:\\Program Files\\Halvo\\HALVO-CAPTURE.EXE\"</Command>";
        assert!(task_registers(
            xml,
            "C:\\Program Files\\Halvo\\halvo-capture.exe"
        ));
        assert!(!task_registers(xml, "C:\\Windows\\notepad.exe"));
        assert!(!task_registers(xml, ""));
    }

    #[test]
    fn create_args_round_trip_the_run_command() {
        let s = spec();
        let args = build_create_args(&s);
        let tr = args.iter().position(|a| a == "/TR").unwrap();
        assert_eq!(args[tr + 1], s.task_run());
    }
}
