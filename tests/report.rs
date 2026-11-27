use std::fs;
use std::path::Path;

use burnlog::analyze::analyze;
use burnlog::render::table;
use burnlog::source::claude_code::{ParseStats, parse};
use serde_json::Value;
use std::io::Cursor;

fn project_with_sessions() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let projects = temp.path().join("projects");
    let project_dir = projects.join(
        cwd.canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('/', "-"),
    );
    fs::create_dir_all(&project_dir).unwrap();
    fs::copy(
        "tests/fixtures/claude-session-basic.jsonl",
        project_dir.join("fixture-session-basic.jsonl"),
    )
    .unwrap();
    fs::copy(
        "tests/fixtures/claude-session-tool.jsonl",
        project_dir.join("fixture-session-tool.jsonl"),
    )
    .unwrap();
    fs::write(
        project_dir.join("fixture-unparsed.jsonl"),
        "{malformed json}\n",
    )
    .unwrap();
    temp
}

fn command(temp: &Path) -> assert_cmd::Command {
    let cwd = temp.join("workspace");
    let projects = temp.join("projects");
    let mut command = assert_cmd::cargo::cargo_bin_cmd!("burnlog");
    command
        .current_dir(cwd)
        .env("BURNLOG_CLAUDE_PROJECTS_DIR", projects);
    command
}

fn total_tokens(session: &Value) -> u64 {
    let totals = &session["token_totals"];
    ["input", "output", "cache_read", "cache_write"]
        .into_iter()
        .map(|key| totals[key].as_u64().unwrap())
        .sum()
}

#[test]
fn cli_reports_package_version() {
    let output = assert_cmd::cargo::cargo_bin_cmd!("burnlog")
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "burnlog 0.1.1\n");
}

#[test]
fn report_table_shows_sections_and_unparsed_footer() {
    let temp = project_with_sessions();
    let output = command(temp.path())
        .args(["report", "fixture-unparsed"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    for section in [
        "TOKENS",
        "CONTEXT GROWTH",
        "TOP TOOL RESULTS",
        "REPEATED READS",
        "FINDINGS",
    ] {
        assert!(stdout.contains(section), "missing {section}: {stdout}");
    }
    assert!(
        stdout.contains("Unparsed lines: 1"),
        "missing unparsed footer: {stdout}"
    );
    assert!(stdout.contains("invalid JSON:"));
}

#[test]
fn report_json_and_session_filter_are_valid() {
    let temp = project_with_sessions();
    let output = command(temp.path())
        .args(["report", "fixture-session-basic", "--json", "--compact"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["session_id"], "fixture-session-basic");
    assert_eq!(json["unparsed_breakdown"]["invalid_json"], 0);
}

#[test]
fn json_includes_unparsed_reasons_as_raw_counts() {
    let temp = project_with_sessions();
    let output = command(temp.path())
        .args(["report", "fixture-unparsed", "--json", "--compact"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["unparsed"], 1);
    assert_eq!(json["unparsed_breakdown"]["invalid_json"], 1);
    assert_eq!(json["token_totals"]["input"], 0);
}

#[test]
fn report_all_sorts_sessions_by_total_tokens_descending() {
    let temp = project_with_sessions();
    let output = command(temp.path())
        .args(["report", "--all", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sessions: Value = serde_json::from_slice(&output.stdout).unwrap();
    let sessions = sessions.as_array().unwrap();
    assert!(
        sessions
            .windows(2)
            .all(|pair| total_tokens(&pair[0]) >= total_tokens(&pair[1]))
    );
}

#[test]
fn missing_log_directory_has_readable_error() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("workspace")).unwrap();
    let output = command(temp.path()).arg("report").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No Claude Code session logs found"));
}

#[test]
fn header_reports_turns_duration_cache_share_and_unknown_resume() {
    let input = fs::read_to_string("tests/fixtures/claude-session-real-structure.jsonl").unwrap();
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    let output = table("synthetic", &report, stats, false);
    assert!(output.contains("Turns: 1"));
    assert!(output.contains("Duration: 5s"));
    assert!(output.contains("Cache-read share: 33.1%"));
    assert!(output.contains("Resumed session: unknown"));
    assert!(output.contains("first observed turn: 151,003"));
    assert!(output.contains("input: 200,001"));
    assert!(!output.contains("initial_context_large"));
}

#[test]
fn tool_sections_explain_empty_results() {
    let report = analyze(&[]);
    let empty = table("empty", &report, ParseStats::default(), false);
    assert!(empty.contains("none found (0 tool blocks parsed)"));

    let (events, stats) = parse(Cursor::new(
        fs::read_to_string("tests/fixtures/claude-session-tool.jsonl").unwrap(),
    ))
    .unwrap();
    let output = table("one-read", &analyze(&events), stats, false);
    assert!(output.contains("none (2 tool blocks parsed, none qualified)"));
}
