use std::io::Cursor;

use burnlog::{analyze::analyze, source::codex::parse};

#[test]
fn mixed_prompt_formats_warn_about_possible_duplicates() {
    let input = r#"{"type":"event_msg","payload":{"type":"user_message","message":"private"}}
{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1"}}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    let json = burnlog::render::json_value("synthetic", &report, stats.clone());
    assert_eq!(json["prompt_count_uncertain"], true);
    let table = burnlog::render::table("synthetic", &report, stats, false);
    assert!(table.contains("include duplicates"));
    assert!(!table.contains("private"));
}

#[test]
fn completed_user_items_and_context_snapshots_are_reported() {
    let input = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1"}}}
{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1"}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":123},"total_token_usage":{"input_tokens":1000,"cached_input_tokens":500,"output_tokens":20}}}}
{"type":"response_item","payload":{"type":"function_call","name":"tool","call_id":"x"}}"#;
    let (events, _) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(report.user_prompts, 1);
    assert_eq!(report.context_growth[0], Some(123));
    assert_eq!(
        events
            .iter()
            .find(|e| e.tool_name.as_deref() == Some("tool"))
            .unwrap()
            .turn_index,
        1
    );
}

#[test]
fn structured_tool_output_only_estimates_text() {
    let input = r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"a","output":[{"type":"input_text","text":"12345678"},{"type":"input_text","text":"abcd"}]}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"b","output":[{"type":"input_text","text":"1234"},{"type":"input_image","image_url":"secret"}]}}"#;
    let (events, _) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(report.top_tool_results[0].size, 3);
    assert_eq!(report.unavailable_tool_result_sizes, 1);
}

#[test]
fn reports_cumulative_usage_once_and_separates_cached_input() {
    let input = r#"{"type":"session_meta","payload":{"id":"synthetic","cwd":"/synthetic"}}
{"type":"event_msg","payload":{"type":"user_message","message":"private prompt"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":10}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":90,"output_tokens":20}}}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(
        (
            report.token_totals.input,
            report.token_totals.cache_read,
            report.token_totals.output
        ),
        (110, 90, 20)
    );
    assert_eq!((report.user_prompts, report.assistant_messages), (1, 1));
    assert_eq!(stats.unparsed, 0);
}

#[test]
fn response_usage_records_are_deduplicated_and_preferred_to_snapshots() {
    let input = r#"{"type":"session_meta","payload":{"id":"synthetic"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":999,"cached_input_tokens":0,"output_tokens":0}}}}
{"type":"token_usage_record","payload":{"response_id":"a","usage":{"input_tokens":100,"cached_input_tokens":30,"output_tokens":7}}}
{"type":"token_usage_record","payload":{"response_id":"a","usage":{"input_tokens":100,"cached_input_tokens":30,"output_tokens":7}}}
{"type":"token_usage_record","payload":{"response_id":"b","usage":{"input_tokens":50,"cached_input_tokens":20,"output_tokens":3}}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let totals = analyze(&events).token_totals;
    assert_eq!(
        (totals.input, totals.cache_read, totals.output),
        (100, 50, 10)
    );
    assert!(stats.usage_mixed_sources);
    let report = analyze(&events);
    let json = burnlog::render::json_value("synthetic", &report, stats.clone());
    assert_eq!(json["usage_coverage_basis"], "response_records");
    assert!(
        burnlog::render::table("synthetic", &report, stats, false)
            .contains("coverage of all responses is unknown")
    );
}

#[test]
fn snapshot_coverage_and_missing_prompts_are_explicit() {
    let input = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":2}}}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    let json = burnlog::render::json_value("synthetic", &report, stats.clone());
    assert_eq!(json["usage_coverage_basis"], "cumulative_snapshot");
    assert_eq!(json["prompt_count_uncertain"], true);
    let table = burnlog::render::table("synthetic", &report, stats, false);
    assert!(table.contains("per-response coverage is unknown"));
    assert!(table.contains("Prompt count may be incomplete"));
}

#[test]
fn missing_codex_usage_is_not_reported_as_zero() {
    let (events, stats) = parse(Cursor::new(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic\"}}\n",
    ))
    .unwrap();
    let report = analyze(&events);
    let json = burnlog::render::json_value("synthetic", &report, stats.clone());
    assert!(json["token_totals"]["input"].is_null());
    assert!(json["token_totals"]["output"].is_null());
    assert!(burnlog::render::table("synthetic", &report, stats, false).contains("input: n/a"));
}

#[test]
fn cache_share_is_unavailable_when_input_is_missing() {
    let input = r#"{"type":"token_usage_record","payload":{"response_id":"a","usage":{"cached_input_tokens":30,"output_tokens":7}}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let value = burnlog::render::json_value("synthetic", &analyze(&events), stats);
    assert!(value["cache_read_share_percent"].is_null());
}

#[test]
fn tool_results_are_linked_and_unknown_nested_records_are_counted() {
    let input = r#"{"type":"response_item","payload":{"type":"function_call","name":"search","call_id":"call-1","arguments":"secret"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"secret output"}}
{"type":"event_msg","payload":{"type":"future_event"}}
{"type":"response_item","payload":{"type":"future_item"}}"#;
    let (events, stats) = parse(Cursor::new(input)).unwrap();
    let report = analyze(&events);
    assert_eq!(
        report.top_tool_results[0].tool_name.as_deref(),
        Some("search")
    );
    assert_eq!(stats.unparsed, 2);
    assert_eq!(stats.unknown_type, 2);
}

#[test]
fn cli_selects_codex_sessions_for_current_project() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let sessions = temp.path().join("codex/sessions/2026/01/01");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&sessions).unwrap();
    let input = format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"synthetic\",\"cwd\":{}}}}}\n{{\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"a\",\"usage\":{{\"input_tokens\":10,\"cached_input_tokens\":4,\"output_tokens\":2}}}}}}\n",
        serde_json::to_string(&project).unwrap()
    );
    std::fs::write(sessions.join("rollout.jsonl"), input).unwrap();
    let output = assert_cmd::cargo::cargo_bin_cmd!("burnlog")
        .current_dir(&project)
        .env("CODEX_HOME", temp.path().join("codex"))
        .args(["report", "--source", "codex", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["session_id"], "synthetic");
    assert_eq!(value["token_totals"]["input"], 6);
    assert!(value["token_totals"]["cache_write"].is_null());
}
