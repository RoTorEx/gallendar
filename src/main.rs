mod config;
mod demo;
mod engine;
mod google;
mod model;
mod update;

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use clap::{Args, CommandFactory, Parser, Subcommand};
use config::Config;
use engine::CalendarApi;
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "gallendar",
    version,
    about = "Synchronize private busy time across shared Google calendars"
)]
struct Cli {
    /// Config path (otherwise GALLENDAR_CONFIG or ~/.x-cli-gallendar/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Machine-readable output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create the config template; interactively open it in the editor
    #[command(visible_alias = "init")]
    Setup {
        /// Write the template without opening an editor
        #[arg(long)]
        no_edit: bool,
    },
    /// Print the config path, or open it with --edit
    Config {
        #[arg(long)]
        edit: bool,
    },
    /// Print gallendar's home directory
    Home,
    /// Print the installed version
    Version,
    /// Install the latest compatible GitHub Release
    Update {
        /// GitHub OWNER/REPO (or GALLENDAR_RELEASE_REPO)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Check source-read and destination-write roles without changing Google data
    Doctor,
    /// Reconcile once; dry-run unless --apply is supplied
    Sync(SyncArgs),
    /// Periodically reconcile; dry-run unless --apply is supplied; Ctrl-C stops
    Run(SyncArgs),
    /// Compare primary and clone free/busy; this does not prove account aggregation
    Availability {
        #[arg(long)]
        account: String,
        /// RFC3339 timestamp with explicit offset
        #[arg(long)]
        start: String,
        /// RFC3339 timestamp with explicit offset
        #[arg(long)]
        end: String,
    },
    /// Run four-calendar synchronization locally with synthetic data (no credentials)
    Demo,
}

#[derive(Args)]
struct SyncArgs {
    /// Apply the displayed changes to Google instead of a dry-run
    #[arg(long, conflicts_with = "dry_run")]
    apply: bool,
    /// Explicitly request the default read-only mode
    #[arg(long)]
    dry_run: bool,
    /// Acknowledge that secondary-calendar availability was checked externally
    #[arg(long, requires = "apply")]
    availability_confirmed: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gallendar: {error:#}");
        std::process::exit(1);
    }
}

fn home() -> Result<PathBuf> {
    let root = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(root).join(".x-cli-gallendar"))
}

fn config_path(cli: &Cli) -> Result<PathBuf> {
    cli.config
        .clone()
        .or_else(|| std::env::var_os("GALLENDAR_CONFIG").map(PathBuf::from))
        .map(Ok)
        .unwrap_or_else(|| Ok(home()?.join("config.toml")))
}

fn print(value: Value, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

fn edit(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    // Treat editor as one executable path; no shell evaluation of config paths.
    let status = std::process::Command::new(editor)
        .arg(path)
        .status()
        .context("Cannot launch editor; set EDITOR to an executable path")?;
    ensure!(status.success(), "Editor exited unsuccessfully");
    Ok(())
}

fn setup(path: &Path, no_edit: bool, as_json: bool) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let created = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(config::EXAMPLE.as_bytes())?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error).context("Cannot create configuration"),
    };
    print(
        json!({ "config": path, "created": created, "existing_config_preserved": !created }),
        as_json,
    )?;
    if !no_edit && !as_json && std::io::stdin().is_terminal() {
        edit(path)?;
    }
    Ok(())
}

fn lock(path: &Path) -> Result<File> {
    let canonical = std::fs::canonicalize(path).context("Cannot resolve config path")?;
    let lock_path = canonical.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    file.try_lock_exclusive()
        .context("Another gallendar process is using this config; stop it first")?;
    Ok(file)
}

fn sync(config: &Config, args: &SyncArgs, as_json: bool) -> Result<()> {
    ensure!(
        !args.apply || args.availability_confirmed,
        "Writing requires --availability-confirmed after the real scheduling check; dry-run needs no acknowledgement"
    );
    let mut api = google::Google::connect(&config.service_account_key)?;
    let planned = engine::plan(&mut api, config, &config.window(Utc::now()))?;
    // Always expose intended changes before mutating. No source contents in output.
    print(
        json!({ "mode": if args.apply { "apply-plan" } else { "dry-run" }, "changes": planned.counts }),
        as_json,
    )?;
    if args.apply {
        engine::apply(&mut api, &planned)?;
        print(
            json!({ "mode": "applied", "changes": planned.counts, "read_back_verified": true }),
            as_json,
        )?;
    }
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let Some(command) = &cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    match command {
        Command::Home => {
            if cli.json {
                return print(json!({ "home": home()? }), true);
            }
            println!("{}", home()?.display());
            return Ok(());
        }
        Command::Version => {
            if cli.json {
                return print(
                    json!({ "name": "gallendar", "version": env!("CARGO_PKG_VERSION") }),
                    true,
                );
            }
            println!("gallendar {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::Update { repo } => {
            ensure!(!cli.json, "update does not support --json");
            return update::run(repo.clone());
        }
        Command::Demo => return print(demo::run()?, cli.json),
        _ => {}
    }
    let path = config_path(&cli)?;
    match command {
        Command::Setup { no_edit } => return setup(&path, *no_edit, cli.json),
        Command::Config { edit: should_edit } => {
            if *should_edit {
                ensure!(!cli.json, "config --edit does not support --json");
                ensure!(path.is_file(), "Config is missing; run gallendar setup");
                edit(&path)?;
            } else {
                if cli.json {
                    return print(json!({ "config": path }), true);
                }
                println!("{}", path.display());
            }
            return Ok(());
        }
        _ => {}
    }
    let config = Config::load(&path)?;
    match command {
        Command::Doctor => {
            let mut api = google::Google::connect(&config.service_account_key)?;
            let namespace = model::digest(&[&config.namespace]);
            let window = config.window(Utc::now());
            let mut checks = Vec::new();
            let mut ready = true;
            for account in config.sources() {
                let source = api.source(&account.source, &window)?;
                let readable = matches!(
                    source.access_role.as_str(),
                    "reader" | "writer" | "writerWithoutPrivateAccess" | "owner"
                );
                ready &= readable;
                checks.push(
                    json!({ "account": account.name, "source_role": source.access_role,
                    "source_readable": readable }),
                );
            }
            for account in config.targets() {
                let target = api.clones(&account.target, &namespace)?;
                let writable = matches!(
                    target.access_role.as_str(),
                    "writer" | "writerWithoutPrivateAccess" | "owner"
                );
                ready &= writable;
                checks.push(
                    json!({ "account": account.name, "target_role": target.access_role,
                    "target_writable": writable }),
                );
            }
            print(
                json!({ "ready": ready, "checks": checks, "google_writes_tested": false,
                "account_availability_validated": false }),
                cli.json,
            )?;
            ensure!(ready, "Insufficient calendar sharing permissions");
            Ok(())
        }
        Command::Sync(args) => {
            let _lock = lock(&path)?;
            sync(&config, args, cli.json)
        }
        Command::Run(args) => {
            ensure!(
                !args.apply || args.availability_confirmed,
                "Writing requires --availability-confirmed"
            );
            let _lock = lock(&path)?;
            let stopped = Arc::new(AtomicBool::new(false));
            let signal = stopped.clone();
            ctrlc::set_handler(move || {
                signal.store(true, Ordering::SeqCst);
            })?;
            while !stopped.load(Ordering::SeqCst) {
                let started = Instant::now();
                if let Err(error) = sync(&config, args, cli.json) {
                    eprintln!("gallendar: sync failed: {error:#}; will retry next pass");
                }
                while !stopped.load(Ordering::SeqCst)
                    && started.elapsed() < Duration::from_secs(config.poll_seconds)
                {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(())
        }
        Command::Availability {
            account,
            start,
            end,
        } => {
            let account = config
                .accounts
                .iter()
                .find(|a| &a.name == account)
                .context("Unknown account name")?;
            let start = DateTime::parse_from_rfc3339(start)
                .context("start must be RFC3339 with an offset")?
                .with_timezone(&Utc);
            let end = DateTime::parse_from_rfc3339(end)
                .context("end must be RFC3339 with an offset")?
                .with_timezone(&Utc);
            ensure!(end > start, "end must be later than start");
            let api = google::Google::connect(&config.service_account_key)?;
            print(
                json!({ "account": account.name,
                "source_busy": if account.source.is_empty() { None } else { Some(api.availability(&account.source, start, end)?) },
                "clone_busy": if account.target.is_empty() { None } else { Some(api.availability(&account.target, start, end)?) },
                "account_aggregation_proven": false }),
                cli.json,
            )
        }
        _ => bail!("Unsupported command"),
    }
}

#[cfg(test)]
mod tests;
