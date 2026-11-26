use std::fs;
use std::path::Path;

use serde_json::Value;

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
fn report_table_shows_sections_and_unparsed_footer() {
    let temp = project_with_sessions();
    let output = command(temp.path()).arg("report").output().unwrap();
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
