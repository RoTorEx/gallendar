use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const LABEL: &str = "com.rotorex.gallendar";

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn plist(binary: &Path, config: &Path, home: &Path) -> Result<String> {
    let text = |path: &Path| -> Result<String> {
        Ok(xml(path
            .to_str()
            .context("Daemon paths must be valid UTF-8")?))
    };
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>--config</string><string>{}</string><string>run</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>30</integer>
<key>ExitTimeOut</key><integer>120</integer>
<key>ProcessType</key><string>Background</string>
<key>WorkingDirectory</key><string>{}</string>
<key>StandardOutPath</key><string>{}</string>
<key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        text(binary)?,
        text(config)?,
        text(home)?,
        text(&home.join("logs/daemon.stdout.log"))?,
        text(&home.join("logs/daemon.stderr.log"))?
    ))
}

fn field(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&format!("{name} = "))
            .map(str::to_owned)
    })
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> Result<std::process::Output> {
    Command::new("/bin/launchctl")
        .args(args)
        .output()
        .context("Cannot run launchctl")
}

#[cfg(target_os = "macos")]
pub fn run(action: &str, config: &Path, home: &Path) -> Result<Value> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let user_home = std::env::var_os("HOME").context("HOME is not set")?;
    let agent = std::path::PathBuf::from(user_home)
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"));
    let uid = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .context("Cannot determine user ID")?;
    ensure!(uid.status.success(), "Cannot determine user ID");
    let domain = format!("gui/{}", String::from_utf8(uid.stdout)?.trim());
    let service = format!("{domain}/{LABEL}");
    let status = || -> Result<Value> {
        let result = launchctl(&["print", &service])?;
        let output = String::from_utf8_lossy(&result.stdout);
        let pid = field(&output, "pid").and_then(|s| s.parse::<u32>().ok());
        Ok(
            json!({"enabled":agent.is_file(), "loaded":result.status.success(),
            "running":result.status.success() && pid.is_some(), "pid":pid,
            "state":field(&output,"state"), "last_exit_code":field(&output,"last exit code"),
            "agent":agent, "logs":home.join("logs")}),
        )
    };
    match action {
        "status" => status(),
        "stop" => {
            if status()?["loaded"] == true {
                let result = launchctl(&["bootout", &service])?;
                ensure!(
                    result.status.success(),
                    "Cannot stop daemon: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                );
            }
            if agent.exists() {
                fs::remove_file(&agent).context("Cannot disable daemon autostart")?;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(125);
            while status()?["loaded"] == true {
                ensure!(
                    std::time::Instant::now() < deadline,
                    "Daemon did not finish stopping; check daemon status and logs"
                );
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            status()
        }
        "start" => {
            let config = fs::canonicalize(config).context("Cannot resolve daemon config")?;
            crate::config::Config::load(&config)?;
            if status()?["loaded"] == true {
                let args = Command::new("/usr/bin/plutil")
                    .args(["-extract", "ProgramArguments", "json", "-o", "-"])
                    .arg(&agent)
                    .output()?;
                let existing: Vec<String> = serde_json::from_slice(&args.stdout)
                    .context("Cannot read existing daemon configuration")?;
                ensure!(
                    existing.get(2).is_some_and(|p| Path::new(p) == config),
                    "Daemon uses another config; run daemon stop first"
                );
                return status();
            }
            // Refuse to compete with an existing foreground sync on this config.
            drop(crate::lock(&config)?);
            let binary = home.join("bin/gallendar");
            ensure!(
                binary.is_file(),
                "Install gallendar in its CLI home before starting the daemon"
            );
            // Never register a temporary build path or a different installed version.
            let installed = Command::new(&binary).arg("--version").output()?;
            ensure!(
                installed.status.success()
                    && String::from_utf8_lossy(&installed.stdout).trim()
                        == format!("gallendar {}", env!("CARGO_PKG_VERSION")),
                "Installed CLI version differs; install this build first"
            );
            fs::create_dir_all(agent.parent().unwrap())?;
            for path in [
                agent.clone(),
                home.join("logs/daemon.stdout.log"),
                home.join("logs/daemon.stderr.log"),
            ] {
                if let Ok(meta) = fs::symlink_metadata(&path) {
                    ensure!(
                        meta.is_file() && !meta.file_type().is_symlink(),
                        "Daemon files must be regular files"
                    );
                }
                let file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .mode(0o600)
                    .open(&path)?;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            fs::write(&agent, plist(&binary, &config, home)?)?;
            let result = launchctl(&[
                "bootstrap",
                &domain,
                agent.to_str().context("Invalid agent path")?,
            ])?;
            if !result.status.success() {
                fs::remove_file(&agent)?;
                anyhow::bail!(
                    "Cannot start daemon: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                );
            }
            let result = status()?;
            ensure!(
                result["loaded"] == true,
                "Daemon was not registered after start"
            );
            Ok(result)
        }
        _ => anyhow::bail!("Unknown daemon action"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn run(_action: &str, _config: &Path, _home: &Path) -> Result<Value> {
    anyhow::bail!("Local daemon requires macOS")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_agent_uses_installed_binary_explicit_config_and_private_logs() {
        let generated = plist(
            Path::new("/home/My CLI/bin/gallendar"),
            Path::new("/home/A&B/config.toml"),
            Path::new("/home/My CLI"),
        )
        .unwrap();
        assert!(generated.contains("<string>/home/My CLI/bin/gallendar</string>"));
        assert!(generated.contains("<string>/home/A&amp;B/config.toml</string>"));
        assert!(generated.contains("<key>KeepAlive</key><true/>"));
        assert!(generated.contains("<key>RunAtLoad</key><true/>"));
        assert!(generated.contains("daemon.stderr.log"));
        assert!(!generated.contains("StartInterval")); // The run loop owns poll_seconds.
    }
    #[test]
    fn launchctl_status_distinguishes_registered_waiting_from_running() {
        assert_eq!(
            field("\tstate = running\n\tpid = 123\n", "pid").as_deref(),
            Some("123")
        );
        assert!(field("state = waiting\nlast exit code = 1\n", "pid").is_none());
        assert_eq!(
            field("last exit code = 1\n", "last exit code").as_deref(),
            Some("1")
        );
    }
}
