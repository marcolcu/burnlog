//! Library entry point for the burnlog CLI.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

pub mod analyze;
pub mod model;
pub mod render;
pub mod source;

#[derive(Parser)]
#[command(name = "burnlog", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze session logs and report token usage.
    Report {
        /// Report all sessions, sorted by total tokens descending.
        #[arg(long, conflicts_with = "session_id")]
        all: bool,
        /// Select a session by its session ID.
        session_id: Option<String>,
        /// Output machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Remove extra whitespace from the output.
        #[arg(long)]
        compact: bool,
    },
}

struct SessionSummary {
    session_id: String,
    report: analyze::SessionReport,
    parse_stats: source::claude_code::ParseStats,
}

pub fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Report {
            all,
            session_id,
            json,
            compact,
        } => run_report(all, session_id.as_deref(), json, compact),
    }
}

fn run_report(all: bool, selected_id: Option<&str>, json: bool, compact: bool) -> Result<()> {
    let paths = source::claude_code::discover_sessions(&std::env::current_dir()?)?;
    if paths.is_empty() {
        bail!("No Claude Code session logs found for the current directory.");
    }

    let mut sessions = paths
        .iter()
        .map(parse_session)
        .collect::<Result<Vec<_>>>()?;
    if all {
        sessions.sort_by_key(|session| std::cmp::Reverse(total_tokens(&session.report)));
    } else if let Some(session_id) = selected_id {
        sessions.retain(|session| session.session_id == session_id);
        if sessions.is_empty() {
            bail!("No Claude Code session found with ID '{session_id}'.");
        }
    } else {
        sessions.truncate(1);
    }

    if json {
        let values: Vec<_> = sessions
            .iter()
            .map(|session| {
                render::json_value(
                    &session.session_id,
                    &session.report,
                    session.parse_stats.clone(),
                )
            })
            .collect();
        let value = if all {
            serde_json::Value::Array(values)
        } else {
            values
                .into_iter()
                .next()
                .context("no session selected for JSON output")?
        };
        let output = if compact {
            serde_json::to_string(&value)?
        } else {
            serde_json::to_string_pretty(&value)?
        };
        println!("{output}");
    } else {
        let reports: Vec<_> = sessions
            .iter()
            .map(|session| {
                render::table(
                    &session.session_id,
                    &session.report,
                    session.parse_stats.clone(),
                    compact,
                )
            })
            .collect();
        println!("{}", reports.join(if compact { "\n" } else { "\n\n" }));
    }
    Ok(())
}

fn parse_session(path: &PathBuf) -> Result<SessionSummary> {
    let file = File::open(path)
        .with_context(|| format!("failed to open session log {}", path.display()))?;
    let (events, stats) = source::claude_code::parse(BufReader::new(file))
        .with_context(|| format!("failed to parse session log {}", path.display()))?;
    let session_id = events
        .iter()
        .find_map(|event| event.session_id.clone())
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let mut report = analyze::analyze(&events);
    if stats.session_id_count > 1 {
        report.resumed_session = analyze::ResumeStatus::Yes;
    }
    Ok(SessionSummary {
        session_id,
        report,
        parse_stats: stats,
    })
}

fn total_tokens(report: &analyze::SessionReport) -> u64 {
    let totals = &report.token_totals;
    totals.input + totals.output + totals.cache_read + totals.cache_write
}
