use std::fs;
use std::path::Path;

use burnlog::analyze::analyze;
use burnlog::render::table;
use burnlog::source::claude_code::{ParseStats, parse};
use serde_json::Value;
use std::io::Cursor;

#[test]
fn data_quality_reports_missing_usage_and_unparsed_share() {
    let input = r#"{"type":"assistant","message":{"id":"a","content":[],"usage":{"input_tokens":0,"output_tokens":2}}}
{"type":"assistant","message":{"id":"a","content":[],"usage":{"input_tokens":0,"output_tokens":2}}}
{"type":"assistant","message":{"id":"b","content":[]}}
not json"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    let value = burnlog::render::json_value("synthetic", &report, stats.clone());
    assert_eq!(value["usage_coverage"]["messages"], 2);
    assert_eq!(value["usage_coverage"]["input"], 1);
    assert_eq!(value["usage_coverage"]["output"], 1);
    assert_eq!(value["usage_coverage"]["cache_read"], 0);
    assert_eq!(value["unparsed_percent"], 25.0);
    let output = table("synthetic", &report, stats, false);
    assert!(output.contains("Usage coverage"));
    assert!(output.contains("25.0%"));
}

#[test]
fn empty_log_has_no_unparsed_percentage() {
    let (events, stats) = parse(Cursor::new("")).unwrap();
    let value = burnlog::render::json_value("empty", &analyze(&events), stats);
    assert!(value["unparsed_percent"].is_null());
}

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
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "burnlog 0.1.4\n");
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
        "REPEATED EDITS",
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
fn header_reports_counts_span_cache_share_and_unknown_resume() {
    let input = fs::read_to_string("tests/fixtures/claude-session-real-structure.jsonl").unwrap();
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    let output = table("synthetic", &report, stats, false);
    assert!(output.contains("User prompts: 1 | Assistant messages: 2 | Tool calls: 2"));
    assert!(output.contains("Span: 0h 0m"));
    assert!(output.contains("Cache-read share: 33.1%"));
    assert!(output.contains("Resumed session: unknown"));
    assert!(output.contains("first observed turn: 151,003"));
    assert!(output.contains("input: 200,001"));
    assert!(output.contains("boundaries detected: 0"));
}

#[test]
fn tool_sections_explain_empty_results() {
    let report = analyze(&[]);
    let empty = table("empty", &report, ParseStats::default(), false);
    assert!(empty.contains("no Read tool calls in this session"));

    let (events, stats) = parse(Cursor::new(
        fs::read_to_string("tests/fixtures/claude-session-tool.jsonl").unwrap(),
    ))
    .unwrap();
    let output = table("one-read", &analyze(&events), stats, false);
    assert!(output.contains("none (1 Read calls, 0 repeated)"));
}

#[test]
fn real_shape_fixture_reports_calls_safe_labels_and_estimated_results() {
    let input = fs::read_to_string("tests/fixtures/claude-session-real-structure-2.jsonl").unwrap();
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == burnlog::model::EventKind::ToolCall)
            .count(),
        6
    );
    assert_eq!(report.tool_calls, 6);
    assert_eq!(report.assistant_messages, 7);
    assert_eq!(report.user_prompts, 2);
    assert_eq!(report.top_tool_results.len(), 3);
    assert_eq!(report.unavailable_tool_result_sizes, 2);
    assert_eq!(report.top_tool_results[0].size, 5);
    assert!(report.top_tool_results[0].estimated);
    assert!(
        report
            .top_tool_results
            .iter()
            .any(|item| item.tool_name.as_deref() == Some("mcp__search")
                && item.label.as_deref() == Some("mcp__search")
                && item.size == 2)
    );
    assert_eq!(report.repeated_edits[0].count, 2);
    assert_eq!(report.repeated_reads[0].count, 2);
    assert_eq!(report.span_seconds, Some(51));
    assert_eq!(report.active_seconds, Some(51));
    assert!(report.context_fact.is_some());
    assert_eq!(stats.ignored, 14);

    let output = table("synthetic", &report, stats, false);
    assert!(output.contains("User prompts: 2 | Assistant messages: 7 | Tool calls: 6"));
    assert!(output.contains("Span: 0h 0m | Active (est.): 0h 0m"));
    assert!(output.contains("curl [url]"));
    assert!(!output.contains("API_KEY"));
    assert!(!output.contains("abc"));
    assert!(!output.contains("x.example"));
    assert!(!output.contains("/workspace"));
    assert!(output.contains("(est.)"));
    assert!(output.contains("size unavailable for 2 blocks"));
    assert!(output.contains("REPEATED EDITS"));
    assert!(output.contains("boundaries detected: 0"));
    assert!(output.contains("+3 more"));
    assert!(output.contains("future-one: 1"));
    assert!(output.contains("Ignored lines: 14"));
}

#[test]
fn json_names_counts_and_marks_estimated_sizes() {
    let input = fs::read_to_string("tests/fixtures/claude-session-real-structure-2.jsonl").unwrap();
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let value = burnlog::render::json_value("synthetic", &analyze(&events), stats);
    assert_eq!(value["user_prompts"], 2);
    assert_eq!(value["assistant_messages"], 7);
    assert_eq!(value["tool_calls"], 6);
    assert_eq!(value["top_tool_results"][0]["estimated"], true);
    assert_eq!(value["top_tool_results"][0]["size"], 5);
    assert_eq!(value["active_seconds"], 51);
    assert_eq!(value["active_seconds_estimated"], true);
    assert_eq!(value["ignored_types"]["custom-title"], 1);
    assert!(value.get("turns").is_none());
    assert!(value.get("duration_seconds").is_none());
    assert_eq!(value["unparsed_breakdown"]["types"]["future-one"], 1);
}
