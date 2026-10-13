mod config;
mod demo;
mod engine;
mod google;
mod logging;
mod model;
mod oauth;
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
    #[command(visible_aliases = ["init", "setup-config"])]
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
        /// Internal background update worker
        #[arg(long, hide = true)]
        automatic: bool,
    },
    /// Sign in per account or inspect locally stored credentials
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Find or explicitly create the globally named destination calendar
    Calendar {
        #[command(subcommand)]
        action: CalendarAction,
    },
    /// Check source-read and destination-write roles without changing Google data
    Doctor,
    /// Reconcile once; preview unless --apply is supplied
    Sync(SyncArgs),
    /// Start synchronization immediately; --dry-run previews; Ctrl-C stops
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

#[derive(Subcommand)]
enum AuthAction {
    /// Sign in to one Google account using a Desktop OAuth client
    Login {
        #[arg(long)]
        account: String,
        /// Print the URL without opening a browser (callback still runs locally)
        #[arg(long)]
        no_browser: bool,
    },
    /// Check local credentials without contacting Google
    Status,
}

#[derive(Subcommand)]
enum CalendarAction {
    /// Reuse the named calendar or create it if absent; never create events
    Ensure {
        #[arg(long)]
        account: String,
    },
}

#[derive(Args)]
struct SyncArgs {
    /// Apply the displayed changes to Google instead of a dry-run
    #[arg(long, conflicts_with = "dry_run")]
    apply: bool,
    /// Preview changes without writing to Google
    #[arg(long)]
    dry_run: bool,
    /// Include mirrored titles, times, destinations and configured guests in the plan
    #[arg(long)]
    details: bool,
}

impl SyncArgs {
    fn writes(&self, periodic: bool) -> bool {
        !self.dry_run && (periodic || self.apply)
    }
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
    logging::event("output", value.clone())?;
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

fn action_details(action: &engine::Action) -> Value {
    match action {
        engine::Action::Create { target, busy } => {
            json!({"action":"create", "target":target, "key":busy.key,
            "title":busy.summary, "start":busy.start, "end":busy.end, "attendees":busy.attendees})
        }
        engine::Action::Update { target, id, busy } => {
            json!({"action":"update", "target":target, "id":id, "key":busy.key,
            "title":busy.summary, "start":busy.start, "end":busy.end, "attendees":busy.attendees})
        }
        engine::Action::Delete { target, id } => {
            json!({"action":"delete", "target":target, "id":id})
        }
    }
}

fn sync(config: &Config, args: &SyncArgs, as_json: bool, apply: bool) -> Result<()> {
    let pass = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();
    let window = config.window(Utc::now());
    logging::event(
        "pass_started",
        json!({"pass":pass,"apply":apply,"start":window.start,"end":window.end}),
    )?;
    let result = sync_pass(config, args, as_json, apply, &pass, &window);
    logging::event(
        if result.is_ok() {
            "pass_completed"
        } else {
            "pass_failed"
        },
        json!({"pass":pass,"duration_ms":started.elapsed().as_millis(),"error":result.as_ref().err().map(|e|format!("{e:#}"))}),
    )?;
    result
}

fn sync_pass(
    config: &Config,
    args: &SyncArgs,
    as_json: bool,
    apply: bool,
    pass: &str,
    window: &config::Window,
) -> Result<()> {
    let mut resolved = config.clone();
    let mut api = google::Google::connect_resolved(&mut resolved)?;
    let config = &resolved;
    let planned = engine::plan(&mut api, config, window)?;
    let events: Vec<_> = planned.actions.iter().map(action_details).collect();
    logging::event(
        "plan",
        json!({"pass":pass,"namespace":planned.namespace,"calendar_name":config.mirror_calendar_name,"changes":planned.counts,"events":events}),
    )?;
    let mut output =
        json!({"mode":if apply {"apply-plan"} else {"dry-run"},"changes":planned.counts});
    if args.details {
        output["events"] = json!(
            events
                .iter()
                .map(|event| {
                    let mut event = event.clone();
                    event["calendar_name"] = json!(config.mirror_calendar_name);
                    event
                })
                .collect::<Vec<_>>()
        );
    }
    print(output, as_json)?;
    if apply {
        engine::apply_with_reporter(&mut api, &planned, |action, stage, error| {
            logging::event(
                stage,
                json!({"pass":pass,"operation":action_details(action),"error":error.map(|e|format!("{e:#}"))}),
            )
        })?;
        print(
            json!({"mode":"applied","changes":planned.counts,"read_back_verified":true}),
            as_json,
        )?;
    }
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.command.is_none() {
        return run_cli(&cli);
    }
    logging::init(&home()?)?;
    let command = match cli.command.as_ref().unwrap() {
        Command::Setup { .. } => "setup",
        Command::Config { .. } => "config",
        Command::Home => "home",
        Command::Version => "version",
        Command::Update { .. } => "update",
        Command::Auth { .. } => "auth",
        Command::Calendar { .. } => "calendar",
        Command::Doctor => "doctor",
        Command::Sync(_) => "sync",
        Command::Run(_) => "run",
        Command::Availability { .. } => "availability",
        Command::Demo => "demo",
    };
    logging::event(
        "command_started",
        json!({"command":command,"version":env!("CARGO_PKG_VERSION"),"config":config_path(&cli)?}),
    )?;
    let result = run_cli(&cli);
    logging::event(
        if result.is_ok() {
            "command_completed"
        } else {
            "command_failed"
        },
        json!({"command":command,"error":result.as_ref().err().map(|e|format!("{e:#}"))}),
    )?;
    result
}

fn run_cli(cli: &Cli) -> Result<()> {
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
        Command::Update { repo, automatic } => {
            ensure!(!cli.json, "update does not support --json");
            return update::run(repo.clone(), *automatic);
        }
        Command::Demo => return print(demo::run()?, cli.json),
        _ => {}
    }
    let path = config_path(cli)?;
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
    if let Command::Auth { action } = command {
        let config = Config::load_for_auth(&path)?;
        return match action {
            AuthAction::Login {
                account,
                no_browser,
            } => {
                ensure!(!cli.json, "auth login does not support --json");
                let _lock = lock(&path)?;
                oauth::login(&config, account, *no_browser)
            }
            AuthAction::Status => print(oauth::status(&config), cli.json),
        };
    }
    let mut config = Config::load(&path)?;
    logging::event(
        "configured",
        json!({"horizon_days":config.horizon_days,"poll_seconds":config.poll_seconds,"mirror_calendar_name":config.mirror_calendar_name,"routes":config.routes()}),
    )?;
    if config.auto_update {
        update::start_automatic();
    }
    match command {
        Command::Calendar {
            action: CalendarAction::Ensure { account },
        } => {
            let _lock = lock(&path)?;
            ensure!(
                config.targets().iter().any(|a| &a.name == account),
                "Account must be a configured route destination"
            );
            let name = config
                .mirror_calendar_name
                .as_deref()
                .context("Set mirror_calendar_name in the global TOML section first")?;
            let mut api = google::Google::connect_config(&config)?;
            let (id, created) = api.named_calendar(account, name, true)?;
            print(
                json!({"account":account,"calendar_name":name,"calendar_id":id,"created":created,"events_created":0}),
                cli.json,
            )
        }
        Command::Doctor => {
            let mut api = google::Google::connect_resolved(&mut config)?;
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
            sync(&config, args, cli.json, args.writes(false))
        }
        Command::Run(args) => {
            let _lock = lock(&path)?;
            let stopped = Arc::new(AtomicBool::new(false));
            let signal = stopped.clone();
            ctrlc::set_handler(move || {
                signal.store(true, Ordering::SeqCst);
            })?;
            while !stopped.load(Ordering::SeqCst) {
                let started = Instant::now();
                if let Err(error) = sync(&config, args, cli.json, args.writes(true)) {
                    logging::event(
                        "retry_scheduled",
                        json!({"error":format!("{error:#}"),"poll_seconds":config.poll_seconds}),
                    )?;
                    eprintln!("gallendar: sync failed: {error:#}; will retry next pass");
                }
                while !stopped.load(Ordering::SeqCst)
                    && started.elapsed() < Duration::from_secs(config.poll_seconds)
                {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            logging::event("stopped", json!({"reason":"Ctrl-C"}))?;
            Ok(())
        }
        Command::Availability {
            account,
            start,
            end,
        } => {
            let api = google::Google::connect_resolved(&mut config)?;
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
            print(
                json!({ "account": account.name,
                "primary_calendar_id": account.owner_email,
                "primary_busy": api.availability(&account.owner_email, start, end)?,
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
