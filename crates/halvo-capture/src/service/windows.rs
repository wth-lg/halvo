//! The Windows background service: a per-user logon Scheduled Task.

use std::process::Command;

use super::{ServiceError, ServiceInstaller, ServiceStatus};

/// The Scheduled Task name.
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

    /// The `/TR` value: the program followed by its arguments, every token
    /// wrapped in literal quotes. Quoting each token (not just the program) keeps
    /// an argument that contains a space one token to the Task Scheduler rather
    /// than two.
    pub fn task_run(&self) -> String {
        let mut run = format!("\"{}\"", self.program);
        for arg in &self.args {
            run.push(' ');
            run.push('"');
            run.push_str(arg);
            run.push('"');
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

/// The text inside each `<Command>` element of a Task Scheduler XML definition.
fn command_texts(xml: &str) -> Vec<&str> {
    const OPEN: &str = "<Command>";
    const CLOSE: &str = "</Command>";
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end + CLOSE.len()..];
    }
    out
}

/// Whether an existing task's XML already registers `program` as its command, so
/// install can skip the rewrite. Reads the `<Command>` element text, not a
/// substring of the whole XML, so a path that appears only in `<Arguments>` or a
/// comment does not count. The match is case-insensitive because Windows paths
/// are, and ignores the surrounding quoting of the command.
pub fn task_registers(existing_xml: &str, program: &str) -> bool {
    if program.is_empty() {
        return false;
    }
    let needle = program.to_ascii_lowercase();
    command_texts(existing_xml).into_iter().any(|cmd| {
        let cmd = cmd.trim().trim_matches('"').trim();
        cmd.to_ascii_lowercase() == needle
    })
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
    fn create_args_are_the_exact_logon_task_vector() {
        let s = spec();
        let mut expected: Vec<String> = [
            "/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/IT", "/TN",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        expected.push(DEFAULT_TASK_NAME.to_string());
        expected.push("/TR".to_string());
        expected.push(s.task_run());
        assert_eq!(build_create_args(&s), expected);
    }

    #[test]
    fn task_run_quotes_every_token() {
        let run = spec().task_run();
        assert_eq!(
            run,
            "\"C:\\Program Files\\Halvo\\halvo-capture.exe\" \"--service-run\""
        );
    }

    #[test]
    fn task_run_keeps_an_arg_with_a_space_as_one_token() {
        let spec = TaskSpec {
            task_name: DEFAULT_TASK_NAME.to_string(),
            program: "C:\\Program Files\\Halvo\\halvo-capture.exe".to_string(),
            args: vec![
                "--log-dir".to_string(),
                "C:\\Program Data\\Halvo".to_string(),
            ],
        };
        // The spaced path is wrapped in one pair of quotes, so Task Scheduler
        // reads it as a single argument rather than splitting on the space.
        assert_eq!(
            spec.task_run(),
            "\"C:\\Program Files\\Halvo\\halvo-capture.exe\" \"--log-dir\" \"C:\\Program Data\\Halvo\""
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
    fn task_registers_matches_the_command_element_case_insensitively() {
        let xml = "<Actions><Exec>\
            <Command>\"C:\\Program Files\\Halvo\\HALVO-CAPTURE.EXE\"</Command>\
            <Arguments>--service-run</Arguments></Exec></Actions>";
        assert!(task_registers(
            xml,
            "C:\\Program Files\\Halvo\\halvo-capture.exe"
        ));
        assert!(!task_registers(xml, "C:\\Windows\\notepad.exe"));
        assert!(!task_registers(xml, ""));
    }

    #[test]
    fn task_registers_ignores_a_program_that_is_only_an_argument() {
        // The program path appears in <Arguments>, not <Command>; a substring
        // search would false-match, a <Command> read must not.
        let xml = "<Actions><Exec>\
            <Command>C:\\Windows\\cmd.exe</Command>\
            <Arguments>C:\\Program Files\\Halvo\\halvo-capture.exe</Arguments>\
            </Exec></Actions>";
        assert!(!task_registers(
            xml,
            "C:\\Program Files\\Halvo\\halvo-capture.exe"
        ));
    }

    #[test]
    fn create_args_round_trip_the_run_command() {
        let s = spec();
        let args = build_create_args(&s);
        let tr = args.iter().position(|a| a == "/TR").unwrap();
        assert_eq!(args[tr + 1], s.task_run());
    }
}
