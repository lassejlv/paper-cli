use std::{
    env,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::{
    fs::{self, OpenOptions},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

#[cfg(not(windows))]
const REPOSITORY_URL: &str = "https://github.com/lassejlv/paper-cli";
#[cfg(not(windows))]
const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/lassejlv/paper-cli/main/install.sh";
#[cfg(windows)]
const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/lassejlv/paper-cli/main/install.ps1";
#[cfg(not(windows))]
const INSTALL_SCRIPT_NAME: &str = "install.sh";
#[cfg(windows)]
const INSTALL_SCRIPT_NAME: &str = "install.ps1";
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
    require_program(
        "npx",
        &["--version"],
        "Node.js and npm are required to update the global use-paper-cli skill",
    )?;
    #[cfg(not(windows))]
    require_program(
        "sh",
        &["-c", "exit 0"],
        "a POSIX shell is required to run the paper CLI installer",
    )?;
    #[cfg(windows)]
    require_program(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", "exit 0"],
        "Windows PowerShell is required to run the paper CLI installer",
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

fn update_cli() -> Result<CliUpdate> {
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
        .with_context(|| format!("failed to download {INSTALL_SCRIPT_NAME}"))?
        .error_for_status()
        .with_context(|| {
            format!("GitHub returned an error while downloading {INSTALL_SCRIPT_NAME}")
        })?
        .bytes()
        .with_context(|| format!("failed to read {INSTALL_SCRIPT_NAME}"))?;

    #[cfg(not(windows))]
    {
        run_install_script(&script, &install_dir)?;
        Ok(CliUpdate { scheduled: false })
    }
    #[cfg(windows)]
    {
        schedule_install_script(&script, &install_dir)?;
        Ok(CliUpdate { scheduled: true })
    }
}

#[cfg(not(windows))]
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

#[cfg(windows)]
fn schedule_install_script(script: &[u8], install_dir: &Path) -> Result<()> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let script_path =
        env::temp_dir().join(format!("paper-cli-upgrade-{}-{nonce}.ps1", process::id()));
    let mut script_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&script_path)
        .with_context(|| {
            format!(
                "failed to create temporary installer {}",
                script_path.display()
            )
        })?;
    script_file.write_all(script).with_context(|| {
        format!(
            "failed to write temporary installer {}",
            script_path.display()
        )
    })?;
    drop(script_file);

    let script_literal = powershell_path_literal(&script_path);
    let command = format!(
        "$ErrorActionPreference = 'Stop'; \
         Wait-Process -Id {} -ErrorAction SilentlyContinue; \
         try {{ & {script_literal} }} \
         finally {{ Remove-Item -LiteralPath {script_literal} -Force -ErrorAction SilentlyContinue }}",
        process::id()
    );
    let spawn_result = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .env("PAPER_INSTALL_DIR", install_dir)
        .env_remove("PAPER_VERSION")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();

    if let Err(error) = spawn_result {
        let _ = fs::remove_file(&script_path);
        return Err(error).context("failed to schedule the Windows paper CLI installer");
    }
    Ok(())
}

#[cfg(windows)]
fn powershell_path_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[derive(Debug, Clone, Copy)]
struct CliUpdate {
    scheduled: bool,
}

fn execute_upgrade(
    mut update_skill: impl FnMut() -> Result<()>,
    mut update_cli: impl FnMut() -> Result<CliUpdate>,
) -> Result<Value> {
    eprintln!("Updating global use-paper-cli skill...");
    update_skill().context("failed to update global use-paper-cli skill")?;

    eprintln!("Updating paper CLI...");
    let cli_update = update_cli().context("failed to update paper CLI")?;
    let cli = if cli_update.scheduled {
        json!({
            "scheduled": true,
            "source": INSTALL_SCRIPT_URL,
            "updated": false
        })
    } else {
        json!({
            "source": INSTALL_SCRIPT_URL,
            "updated": true
        })
    };

    Ok(json!({
        "cli": cli,
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
                Ok(CliUpdate { scheduled: false })
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
                Ok(CliUpdate { scheduled: false })
            },
        )
        .unwrap_err();

        assert_eq!(attempts.get(), 1);
        assert!(error.to_string().contains("global use-paper-cli skill"));
    }

    #[test]
    fn scheduled_cli_update_is_reported_without_claiming_completion() {
        let result = execute_upgrade(|| Ok(()), || Ok(CliUpdate { scheduled: true }))
            .expect("upgrade succeeds");

        assert_eq!(result["skill"]["updated"], true);
        assert_eq!(result["cli"]["scheduled"], true);
        assert_eq!(result["cli"]["updated"], false);
    }

    #[cfg(not(windows))]
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
