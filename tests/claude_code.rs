use std::fs;
use std::io::Cursor;
use std::path::Path;

use burnlog::analyze::analyze;
use burnlog::model::EventKind;
use burnlog::source::claude_code::{discover_sessions_with_root, parse};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new("tests/fixtures").join(name)).unwrap()
}

#[test]
fn parses_message_usage_tool_events_and_boundary() {
    let (basic, basic_stats) = parse(Cursor::new(fixture("claude-session-basic.jsonl"))).unwrap();
    assert_eq!(basic_stats.parsed, 2);
    assert_eq!(basic_stats.unparsed, 0);
    assert_eq!(basic.len(), 2);
    assert_eq!(basic[0].kind, EventKind::UserPrompt);
    assert_eq!(
        basic[0].session_id.as_deref(),
        Some("fixture-session-basic")
    );
    assert_eq!(basic[0].turn_index, 1);
    assert_eq!(basic[1].kind, EventKind::AssistantMessage);
    assert_eq!(basic[1].usage.input_tokens, Some(12));
    assert_eq!(basic[1].usage.cache_creation_input_tokens, Some(2));
    assert_eq!(
        basic[1].timestamp.as_deref(),
        Some("2026-01-01T00:00:01.000Z")
    );

    let (tool, tool_stats) = parse(Cursor::new(fixture("claude-session-tool.jsonl"))).unwrap();
    assert_eq!(tool_stats.parsed, 2);
    assert_eq!(tool.len(), 2);
    assert_eq!(tool[0].kind, EventKind::ToolCall);
    assert_eq!(tool[0].tool_name.as_deref(), Some("Read"));
    assert_eq!(
        tool[0].target_path.as_deref(),
        Some("/workspace/synthetic/example.txt")
    );
    assert_eq!(tool[0].usage.output_tokens, Some(3));
    assert_eq!(tool[1].kind, EventKind::ToolResult);
    assert_eq!(tool[1].tool_name.as_deref(), Some("Read"));

    let (boundary, _) = parse(Cursor::new(fixture("claude-session-compact.jsonl"))).unwrap();
    assert_eq!(boundary.len(), 1);
    assert_eq!(boundary[0].kind, EventKind::Boundary);
}

#[test]
fn malformed_line_is_counted_as_unparsed() {
    let input = format!("{{bad json}}\n{}", fixture("claude-session-basic.jsonl"));
    let (_, stats) = parse(Cursor::new(input)).unwrap();
    assert_eq!(stats.parsed, 2);
    assert_eq!(stats.invalid_json, 1);
    assert_eq!(stats.unparsed, 1);
}

#[test]
fn unknown_record_type_is_skipped_and_counted() {
    let input = r#"{"type":"future-event","sessionId":"s"}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    assert!(events.is_empty());
    assert_eq!(stats.parsed, 0);
    assert_eq!(stats.unknown_type, 1);
    assert_eq!(stats.unparsed, 1);
}

#[test]
fn exact_duplicate_assistant_record_does_not_repeat_usage() {
    let contents = fixture("claude-session-basic.jsonl");
    let assistant = contents
        .lines()
        .find(|line| line.contains("\"type\":\"assistant\""))
        .unwrap();
    let input = format!("{assistant}\n{assistant}\n");
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(stats.parsed, 2);
    assert_eq!(stats.unparsed, 0);
}

#[test]
fn parses_real_nested_tool_blocks_and_deduplicates_message_ids() {
    let contents = fixture("claude-session-real-structure.jsonl");
    let (events, stats) = parse(Cursor::new(contents.clone())).unwrap();
    assert!(stats.unparsed * 20 < contents.lines().count());
    assert_eq!(stats.ignored, 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == EventKind::ToolResult)
            .count(),
        2
    );
    let report = analyze(&events);
    assert_eq!(report.repeated_reads.len(), 1);
    assert_eq!(report.repeated_reads[0].count, 2);
    assert_eq!(report.token_totals.input, 200_001);
    assert_eq!(
        report.resumed_session,
        burnlog::analyze::ResumeStatus::Unknown
    );
    assert!(report.context_fact.is_some());
}

#[test]
fn multiple_session_ids_are_resume_evidence() {
    let input = r#"{"type":"assistant","sessionId":"one","message":{"id":"m1","content":[],"usage":{"input_tokens":1}}}
{"type":"assistant","sessionId":"two","message":{"id":"m2","content":[],"usage":{"input_tokens":1}}}"#;
    let (events, _) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(report.resumed_session, burnlog::analyze::ResumeStatus::Yes);
}

#[test]
fn session_ids_on_ignored_metadata_count_as_resume_evidence() {
    let input = r#"{"type":"assistant","sessionId":"one","message":{"id":"m1","content":[]}}
{"type":"attachment","session_id":"two"}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    assert_eq!(stats.session_id_count, 2);
    assert_eq!(
        analyze(&events).resumed_session,
        burnlog::analyze::ResumeStatus::Unknown
    );
}

#[test]
fn parse_stats_break_down_unparsed_reasons() {
    let input = "{bad json}\n[]\n{\"type\":\"future-event\"}\n{\"type\":\"assistant\"}\n";
    let (_, stats) = parse(Cursor::new(input)).unwrap();
    assert_eq!(stats.invalid_json, 1);
    assert_eq!(stats.other, 1);
    assert_eq!(stats.unknown_type, 1);
    assert_eq!(stats.deserialize_failed, 1);
    assert_eq!(stats.unparsed, 4);
}

#[test]
fn documented_metadata_type_is_ignored() {
    let (_, stats) = parse(Cursor::new(r#"{"type":"attachment"}"#)).unwrap();
    assert_eq!(stats.ignored, 1);
    assert_eq!(stats.unparsed, 0);
}

#[test]
fn discovers_project_sessions_newest_first() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = Path::new("/workspace/project");
    let project_dir = temp.path().join("-workspace-project");
    fs::create_dir(&project_dir).unwrap();
    let older = project_dir.join("older.jsonl");
    let newer = project_dir.join("newer.jsonl");
    fs::write(&older, "").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&newer, "").unwrap();

    let sessions = discover_sessions_with_root(cwd, temp.path()).unwrap();
    assert_eq!(sessions, vec![newer, older]);
}
