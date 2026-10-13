use crate::config::Config;
use anyhow::Result;
use chrono::DateTime;
use serde_json::{Value, json};
use std::path::Path;

pub fn run(path: &Path, home: &Path, as_json: bool) -> Result<()> {
    let daemon = crate::daemon::run("status", path, home)?;
    let configuration = match Config::load(path) {
        Ok(config) => json!({"valid":true,"auth":config.auth,
            "horizon_days":config.horizon_days,"poll_seconds":config.poll_seconds,
            "auto_update":config.auto_update,"routes":config.routes()}),
        Err(_) => {
            json!({"valid":false,"error":if path.is_file() {"Invalid configuration"} else {"Configuration is missing"}})
        }
    };
    let report = json!({"version":env!("CARGO_PKG_VERSION"),"home":home,"config":path,
        "configuration":configuration,"daemon":daemon});
    if as_json {
        return crate::print(report, true);
    }
    crate::logging::event("output", report.clone())?;
    println!("{}", render(&report));
    Ok(())
}

fn timestamp(value: &Value) -> String {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|at| {
            at.with_timezone(&chrono_tz::Europe::Warsaw)
                .format("%Y-%m-%d %H:%M:%S Warsaw (%Z, UTC%:z)")
                .to_string()
        })
        .unwrap_or_else(|| "unknown time".into())
}

fn render(report: &Value) -> String {
    let mut lines = vec![format!(
        "Gallendar {}",
        report["version"].as_str().unwrap_or("unknown")
    )];
    let daemon = &report["daemon"];
    let running = daemon["running"] == true;
    let state = if running {
        let version = daemon["version"]
            .as_str()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "version unknown".into());
        format!("running · {version} · PID {}", daemon["pid"])
    } else if daemon["loaded"] == true {
        "waiting".into()
    } else {
        "stopped".into()
    };
    lines.push(format!("Daemon     {state}"));
    lines.push(format!(
        "Autostart  {}",
        if daemon["enabled"] == true {
            "enabled"
        } else {
            "disabled"
        }
    ));
    let config = &report["configuration"];
    if config["valid"] == true {
        let routes = config["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                format!(
                    "{} → {}",
                    r["from"].as_str().unwrap_or("?"),
                    r["to"].as_str().unwrap_or("?")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!("Sync       {routes}"));
        lines.push(format!(
            "Auth       {}",
            config["auth"].as_str().unwrap_or("unknown")
        ));
        let seconds = config["poll_seconds"].as_u64().unwrap_or(0);
        let interval = if seconds > 0 && seconds.is_multiple_of(60) {
            format!("{} min", seconds / 60)
        } else {
            format!("{seconds} s")
        };
        lines.push(format!(
            "Window     {} days · every {interval}",
            config["horizon_days"]
        ));
        lines.push(format!(
            "Updates    {}",
            if config["auto_update"] == true {
                "automatic"
            } else {
                "manual"
            }
        ));
    } else {
        lines.push(format!(
            "Config     {}",
            config["error"].as_str().unwrap_or("unavailable")
        ));
    }
    let last = &daemon["sync"]["last_pass"];
    if last.is_object() {
        let outcome = if last["error"].is_null() {
            "OK"
        } else {
            "FAILED"
        };
        lines.push(format!(
            "Last sync  {outcome} · {}",
            timestamp(&last["timestamp"])
        ));
        if let Some(error) = last["error"].as_str() {
            lines.push(format!("Error      {error}"));
        }
    } else {
        lines.push("Last sync  no applied periodic pass recorded".into());
    }
    lines.push(format!(
        "Config     {}",
        report["config"].as_str().unwrap_or("unknown")
    ));
    if daemon["config"] != report["config"] {
        lines.push(format!(
            "Daemon cfg {}",
            daemon["config"].as_str().unwrap_or("unknown")
        ));
    }
    lines.push(format!(
        "Logs       {}",
        daemon["logs"].as_str().unwrap_or("unknown")
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> Value {
        json!({"version":"0.2.3","config":"/config.toml","configuration":{"valid":true,
            "horizon_days":5,"poll_seconds":600,"auto_update":true,
            "routes":[{"from":"aveni","to":"innowise"}]},
            "daemon":{"running":true,"loaded":true,"enabled":true,"pid":123,"version":"0.2.2",
                "config":"/config.toml","logs":"/logs","sync":{"last_pass":{
                    "timestamp":"2026-10-13T18:00:00Z","error":null}}}})
    }
    #[test]
    fn status_distinguishes_worker_version_and_formats_warsaw_time() {
        let text = render(&report());
        assert!(text.contains("Gallendar 0.2.3"));
        assert!(text.contains("running · v0.2.2 · PID 123"));
        assert!(text.contains("20:00:00 Warsaw (CEST, UTC+02:00)"));
        assert!(text.contains("aveni → innowise"));
        assert!(text.contains("5 days · every 10 min"));
        assert!(
            timestamp(&json!("2026-01-13T18:00:00Z")).contains("19:00:00 Warsaw (CET, UTC+01:00)")
        );
    }
    #[test]
    fn status_handles_unknown_worker_stopped_missing_config_and_failures() {
        let mut data = report();
        data["daemon"]["version"] = Value::Null;
        assert!(render(&data).contains("version unknown"));
        data["daemon"]["running"] = json!(false);
        data["daemon"]["loaded"] = json!(false);
        data["configuration"] = json!({"valid":false,"error":"Configuration is missing"});
        data["daemon"]["sync"]["last_pass"]["error"] = json!("network timeout");
        let text = render(&data);
        assert!(text.contains("Daemon     stopped"));
        assert!(text.contains("Configuration is missing"));
        assert!(text.contains("FAILED"));
        assert!(text.contains("network timeout"));
        assert!(!text.contains("v0.2.2"));
    }
}
