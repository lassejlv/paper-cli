use std::{
    env,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const REPOSITORY_URL: &str = "https://github.com/lassejlv/paper-cli";
const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/lassejlv/paper-cli/main/install.sh";
const SKILL_NAME: &str = "use-paper-cli";
const SKILL_UPDATE_ARGUMENTS: &[&str] = &[
    "--yes",
    "skills@latest",
    "update",
    SKILL_NAME,
    "--global",
    "--yes",
];

pub fn run() -> Result<Value> {
    if cfg!(windows) {
        bail!(
            "automatic CLI upgrades are not supported on Windows yet; download the latest \
             Windows archive from {REPOSITORY_URL}/releases/latest"
        );
    }

    require_program(
        "npx",
        &["--version"],
        "Node.js and npm are required to update the global use-paper-cli skill",
    )?;
    require_program(
        "sh",
        &["-c", "exit 0"],
        "a POSIX shell is required to run the paper CLI installer",
    )?;
    execute_upgrade(update_skill, update_cli)
}

fn require_program(program: &str, arguments: &[&str], requirement: &str) -> Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("`{program}` is not available; {requirement}"))?;

    if !status.success() {
        bail!("`{program}` prerequisite check failed; {requirement}");
    }
    Ok(())
}

fn update_skill() -> Result<()> {
    let status = Command::new("npx")
        .args(SKILL_UPDATE_ARGUMENTS)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start the global use-paper-cli skill update")?;

    if !status.success() {
        bail!(
            "global use-paper-cli skill update failed with {status}; retry with \
             `npx --yes skills@latest update use-paper-cli --global --yes`"
        );
    }
    Ok(())
}

fn update_cli() -> Result<()> {
    let install_dir = env::current_exe()
        .context("failed to locate the running paper executable")?
        .parent()
        .context("the running paper executable has no parent directory")?
        .to_owned();
    let script = reqwest::blocking::Client::builder()
        .user_agent(concat!("paper-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to prepare the installer download")?
        .get(INSTALL_SCRIPT_URL)
        .send()
        .context("failed to download install.sh")?
        .error_for_status()
        .context("GitHub returned an error while downloading install.sh")?
        .bytes()
        .context("failed to read install.sh")?;

    run_install_script(&script, &install_dir)
}

fn run_install_script(script: &[u8], install_dir: &Path) -> Result<()> {
    let mut child = Command::new("sh")
        .arg("-s")
        .env("PAPER_INSTALL_DIR", install_dir)
        .env_remove("PAPER_VERSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start install.sh")?;

    let write_result = child
        .stdin
        .take()
        .context("failed to open install.sh input")?
        .write_all(script);
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).context("failed to send install.sh to the shell");
    }

    let status = child.wait().context("failed to wait for install.sh")?;
    if !status.success() {
        bail!(
            "paper CLI update failed with {status}; retry the installer from \
             {REPOSITORY_URL}#install"
        );
    }
    Ok(())
}

fn execute_upgrade(
    mut update_skill: impl FnMut() -> Result<()>,
    mut update_cli: impl FnMut() -> Result<()>,
) -> Result<Value> {
    eprintln!("Updating global use-paper-cli skill...");
    update_skill().context("failed to update global use-paper-cli skill")?;

    eprintln!("Updating paper CLI...");
    update_cli().context("failed to update paper CLI")?;

    Ok(json!({
        "cli": {
            "source": INSTALL_SCRIPT_URL,
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
    use std::cell::{Cell, RefCell};

    #[test]
    fn upgrade_runs_skill_then_cli_and_reports_both() {
        let updates = RefCell::new(Vec::new());

        let result = execute_upgrade(
            || {
                updates.borrow_mut().push("skill");
                Ok(())
            },
            || {
                updates.borrow_mut().push("cli");
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(*updates.borrow(), ["skill", "cli"]);
        assert_eq!(result["skill"]["updated"], true);
        assert_eq!(result["skill"]["scope"], "global");
        assert_eq!(result["cli"]["updated"], true);
        assert_eq!(result["cli"]["source"], INSTALL_SCRIPT_URL);
    }

    #[test]
    fn upgrade_stops_when_a_step_fails() {
        let attempts = Cell::new(0);
        let error = execute_upgrade(
            || {
                attempts.set(attempts.get() + 1);
                bail!("update command failed")
            },
            || {
                attempts.set(attempts.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(attempts.get(), 1);
        assert!(error.to_string().contains("global use-paper-cli skill"));
    }

    #[cfg(unix)]
    #[test]
    fn installer_receives_the_target_directory() {
        let directory = tempfile::tempdir().unwrap();
        run_install_script(
            b"touch \"$PAPER_INSTALL_DIR/installer-ran\"\n",
            directory.path(),
        )
        .unwrap();

        assert!(directory.path().join("installer-ran").is_file());
    }
}
