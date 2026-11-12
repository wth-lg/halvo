//! `agent-config presets` prints the preset of each seed category with the answer of `validate`, and the count;
//! `agent-config validate FILE` prints the answer for one record of `agent_config` (A2.5). Exit 0 when every
//! answer is `valid`, 1 for a cause, 2 for a refusal, which prints one line `REFUSED <cause>: <detail>` on
//! standard error and nothing on standard output; the causes of a refusal are `usage` and `file`.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use halvo_policy::{preset, validate};
use halvo_schema::executable::{AgentConfig, Autonomy};
use halvo_schema::{AgentId, Category, SeedCategory, WorkflowId};

const USAGE: &str = "agent-config presets | agent-config validate FILE";

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn answer(config: &AgentConfig) -> &'static str {
    match validate(config) {
        Ok(()) => "valid",
        Err(cause) => cause.as_str(),
    }
}

fn presets() -> ExitCode {
    let workflow = WorkflowId::from_random([0x30; 16]);
    let agent = AgentId::from_random([0x40; 16]);
    let mut lines = Vec::new();
    let mut valid = 0;
    let mut unattended = 0;
    for &category in SeedCategory::ALL {
        let Some(config) = preset(Category::from(category), workflow.clone(), agent.clone()) else {
            lines.push(format!(
                "preset category={} runtime=- autonomy=- result=none",
                category.as_str()
            ));
            continue;
        };
        let result = answer(&config);
        if result == "valid" {
            valid += 1;
        }
        if config.autonomy == Autonomy::Unattended {
            unattended += 1;
        }
        lines.push(format!(
            "preset category={} runtime={} autonomy={} result={result}",
            category.as_str(),
            config.runtime.as_str(),
            config.autonomy.as_str()
        ));
    }
    lines.push(format!(
        "presets seeds={} valid={valid} unattended={unattended}",
        SeedCategory::ALL.len()
    ));
    println!("{}", lines.join("\n"));
    if presets_pass(valid, unattended) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn presets_pass(valid: usize, unattended: usize) -> bool {
    valid == SeedCategory::ALL.len() && unattended == 0
}

fn validate_file(path: &Path) -> ExitCode {
    let Ok(bytes) = std::fs::read(path) else {
        return refuse("file", "the file cannot be read");
    };
    let result = match AgentConfig::parse(&bytes) {
        Ok(config) => answer(&config),
        Err(_) => "malformed",
    };
    println!("config result={result}");
    if result == "valid" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command] if command == "presets" => presets(),
        [command, path] if command == "validate" => validate_file(Path::new(path)),
        _ => refuse("usage", USAGE),
    }
}

#[cfg(test)]
mod tests {
    use super::presets_pass;

    #[test]
    fn presets_pass_only_with_ten_valid_and_none_unattended() {
        assert!(presets_pass(10, 0));
        assert!(!presets_pass(10, 1));
        assert!(!presets_pass(9, 0));
        assert!(!presets_pass(11, 0));
    }
}
