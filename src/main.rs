//! Enough Internet for Today: a finite digest in three zones, Slack, Email and Social, which the
//! herdr-app `digest` mod shows.

mod collect;
mod error;
mod files;
mod helper;
mod legacy;
mod schedule;
mod sources;
mod x;
mod x_auth;
mod zone;

/// Taken for writing by a unit test that spawns a process, and for reading by those that take
/// the worker lock: a process spawned from the test binary holds copies of its open files, locks
/// included, until it runs its program, so a test's lock could look held by another run.
#[cfg(test)]
static SPAWNING: std::sync::RwLock<()> = std::sync::RwLock::new(());

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "herdr-digest",
    about = "Enough Internet for Today: a finite digest of Slack, email and social media"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Collect the zones; the timer runs it with --due, the panel's Refresh with --zone.
    Collect {
        /// Only collect the zones whose scheduled slot is due.
        #[arg(long)]
        due: bool,
        /// The zones to collect, all of them by default.
        #[arg(long, value_enum)]
        zone: Vec<zone::Zone>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        data: Option<PathBuf>,
    },
    /// Write the `enough-internet` hc template that runs the Sonnet helper.
    Configure {
        /// Let the helper search private Slack channels and DMs.
        #[arg(long)]
        include_private_slack: bool,
        /// Keep the private Slack scope this config sets.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Authorize X read access in the browser, then save its tokens.
    Authorize {
        #[arg(long)]
        store: Option<PathBuf>,
        #[arg(long)]
        secrets_file: Option<PathBuf>,
        /// Native app: do not use a client secret.
        #[arg(long)]
        public_client: bool,
    },
}

fn default_config() -> PathBuf {
    files::home().join(".config/herdr-digest/config.json")
}

fn default_data() -> PathBuf {
    std::env::var_os("HERDR_APP_HOME")
        .map_or_else(|| files::home().join(".config/herdr-app"), PathBuf::from)
        .join("data/digest")
}

/// `herdr-digest collect`: every zone unless `zones` names some.
fn collect(
    due: bool,
    zones: Vec<zone::Zone>,
    config: Option<PathBuf>,
    data: Option<PathBuf>,
) -> ExitCode {
    let config = config.unwrap_or_else(default_config);
    let data = data.unwrap_or_else(default_data);
    let zones = if zones.is_empty() {
        zone::Zone::ALL.to_vec()
    } else {
        zones
    };
    match collect::run(&config, &data, &zones, due, &mut collect::Live) {
        Ok(collect::Run::Failed) => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Commands::Collect {
            due,
            zone,
            config,
            data,
        } => collect(due, zone, config, data),
        Commands::Configure {
            include_private_slack,
            config,
        } => {
            let configured = (|| {
                let mut private = include_private_slack;
                if let Some(config) = config
                    && let Some(settings) = files::read(&config)?
                {
                    private |= files::truthy(settings.get("slack_include_private"));
                }
                helper::configure(&files::home(), private)
            })();
            match configured {
                Ok(template) => {
                    println!("{}", template.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e:#}");
                    ExitCode::FAILURE
                }
            }
        }
        Commands::Authorize {
            store,
            secrets_file,
            public_client,
        } => {
            let store = store.unwrap_or_else(x_auth::default_store);
            let secrets = secrets_file.unwrap_or_else(x_auth::default_secrets);
            let authorized = store
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(anyhow::Error::from)
                .and_then(|()| x_auth::authorize(&store, &secrets, public_client));
            match authorized {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    if error::is_shown(&e) {
                        println!("{e}");
                    } else {
                        println!(
                            "Authorization could not start; check callback port and file access"
                        );
                    }
                    ExitCode::FAILURE
                }
            }
        }
    }
}
