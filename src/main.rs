mod db;
mod popup;
mod timer;
mod tray;

use anyhow::Result;
use chrono::{Duration, Local, TimeZone};
use clap::{Parser, Subcommand};
use db::Database;

#[derive(Parser)]
#[command(name = "pomo", about = "A fast terminal Pomodoro & time tracker")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a focused Pomodoro session in the terminal
    Start {
        /// Project or task tag (e.g. "rust", "writing")
        #[arg(default_value = "general")]
        project: String,

        /// Duration in minutes
        #[arg(short, long, default_value_t = 25)]
        mins: u64,
    },
    /// View total logged time grouped by project
    Stats {
        /// Show only sessions logged today (since midnight)
        #[arg(long, conflicts_with = "week")]
        today: bool,

        /// Show only sessions logged in the last 7 days
        #[arg(long, conflicts_with = "today")]
        week: bool,
    },
    /// Launch the native macOS menu bar timer
    Tray,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = Database::init()?;

    match cli.command {
        Some(Commands::Start { project, mins }) => {
            if let Some(logged_mins) = timer::run_timer(&project, mins)? {
                db.log_session(&project, logged_mins)?;
            }
        }
        Some(Commands::Stats { today, week }) => {
            let now = Local::now();

            let since = if today {
                let midnight = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
                Local.from_local_datetime(&midnight).single()
            } else if week {
                Some(now - Duration::days(7))
            } else {
                None
            };

            let stats = db.get_stats(since)?;
            if stats.is_empty() {
                println!("No sessions found for this time period.");
            } else {
                println!("{:<15} | {:<10} | {:<10}", "PROJECT", "SESSIONS", "TOTAL TIME");
                println!("{}", "-".repeat(41));
                for s in stats {
                    let hours = s.total_minutes / 60;
                    let mins = s.total_minutes % 60;
                    println!(
                        "{:<15} | {:<10} | {}h {}m",
                        s.project, s.session_count, hours, mins
                    );
                }
            }
        }
        // Default to tray mode when launched with no arguments (e.g., from Pomo.app)
        Some(Commands::Tray) | None => {
            tray::run_tray(db)?;
        }
    }

    Ok(())
}