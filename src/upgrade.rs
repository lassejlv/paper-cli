use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const REPOSITORY_URL: &str = "https://github.com/lassejlv/paper-cli";
const SKILL_NAME: &str = "use-paper-cli";

#[derive(Debug, Clone, Copy)]
struct UpgradeStep {
    label: &'static str,
    program: &'static str,
    arguments: &'static [&'static str],
    retry_command: &'static str,
}

const UPGRADE_STEPS: [UpgradeStep; 2] = [
    UpgradeStep {
        label: "global use-paper-cli skill",
        program: "npx",
        arguments: &[
            "--yes",
            "skills@latest",
            "update",
            SKILL_NAME,
            "--global",
            "--yes",
        ],
        retry_command: "npx --yes skills@latest update use-paper-cli --global --yes",
    },
    UpgradeStep {
        label: "paper CLI",
        program: "cargo",
        arguments: &["install", "--git", REPOSITORY_URL, "--locked", "--force"],
        retry_command: "cargo install --git https://github.com/lassejlv/paper-cli --locked --force",
    },
];

pub fn run() -> Result<Value> {
    require_program(
        "npx",
        "Node.js and npm are required to update the global use-paper-cli skill",
    )?;
    require_program(
        "cargo",
        "a current Rust toolchain is required to update the paper CLI",
    )?;
    execute_upgrade(run_step)
}

fn require_program(program: &str, requirement: &str) -> Result<()> {
    let status = Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("`{program}` is not available; {requirement}"))?;

    if !status.success() {
        bail!("`{program} --version` failed; {requirement}");
    }
    Ok(())
}

fn run_step(step: &UpgradeStep) -> Result<()> {
    let status = Command::new(step.program)
        .args(step.arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to start the {} update", step.label))?;

    if !status.success() {
        bail!(
            "{} update failed with {status}; retry with `{}`",
            step.label,
            step.retry_command
        );
    }
    Ok(())
}

fn execute_upgrade(mut execute: impl FnMut(&UpgradeStep) -> Result<()>) -> Result<Value> {
    for step in &UPGRADE_STEPS {
        eprintln!("Updating {}...", step.label);
        execute(step).with_context(|| format!("failed to update {}", step.label))?;
    }

    Ok(json!({
        "cli": {
            "source": REPOSITORY_URL,
            "updated": true
        },
        "skill": {
            "name": SKILL_NAME,
            "scope": "global",
            "updated": true
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_runs_skill_then_cli_and_reports_both() {
        let mut commands = Vec::new();

        let result = execute_upgrade(|step| {
            commands.push((step.program, step.arguments));
            Ok(())
        })
        .unwrap();

        assert_eq!(
            commands,
            vec![
                (
                    "npx",
                    &[
                        "--yes",
                        "skills@latest",
                        "update",
                        "use-paper-cli",
                        "--global",
                        "--yes"
                    ][..]
                ),
                (
                    "cargo",
                    &[
                        "install",
                        "--git",
                        "https://github.com/lassejlv/paper-cli",
                        "--locked",
                        "--force"
                    ][..]
                )
            ]
        );
        assert_eq!(result["skill"]["updated"], true);
        assert_eq!(result["skill"]["scope"], "global");
        assert_eq!(result["cli"]["updated"], true);
    }

    #[test]
    fn upgrade_stops_when_a_step_fails() {
        let mut attempts = 0;
        let error = execute_upgrade(|_| {
            attempts += 1;
            bail!("update command failed")
        })
        .unwrap_err();

        assert_eq!(attempts, 1);
        assert!(error.to_string().contains("global use-paper-cli skill"));
    }
}
