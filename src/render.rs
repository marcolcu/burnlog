//! Report output formatting.

use serde_json::{Value, json};

use crate::analyze::{SessionReport, TOP_TOOL_RESULT_LIMIT};
use crate::source::claude_code::ParseStats;

pub fn json_value(session_id: &str, report: &SessionReport, stats: ParseStats) -> Value {
    json!({
        "session_id": session_id,
        "user_prompts": report.user_prompts,
        "assistant_messages": report.assistant_messages,
        "tool_calls": report.tool_calls,
        "span_seconds": report.span_seconds,
        "active_seconds": report.active_seconds,
        "active_seconds_estimated": true,
        "resumed_session": match report.resumed_session { crate::analyze::ResumeStatus::Yes => "yes", crate::analyze::ResumeStatus::No => "no", crate::analyze::ResumeStatus::Unknown => "unknown" },
        "cache_read_share_percent": cache_read_share(report),
        "token_totals": {
            "input": report.token_totals.input,
            "output": report.token_totals.output,
            "cache_read": report.token_totals.cache_read,
            "cache_write": report.token_totals.cache_write,
        },
        "context_growth": report.context_growth,
        "initial_context_size": report.initial_context_size,
        "context_fact": report.context_fact.as_ref().map(|fact| json!({
            "calls": fact.calls,
            "assistant_calls": fact.assistant_calls,
            "cache_read_percent": fact.cache_read_percent,
            "first_turn": fact.first_turn,
        })),
        "boundary_count": report.boundary_count,
        "top_tool_results": report.top_tool_results.iter().take(TOP_TOOL_RESULT_LIMIT).map(|item| json!({
            "tool_name": item.tool_name,
            "label": item.label,
            "turn": item.turn,
            "size": item.size,
            "estimated": item.estimated,
        })).collect::<Vec<_>>(),
        "unavailable_tool_result_sizes": report.unavailable_tool_result_sizes,
        "repeated_reads": repeated_json(&report.repeated_reads),
        "repeated_edits": repeated_json(&report.repeated_edits),
        "unparsed": stats.unparsed,
        "unparsed_breakdown": {
            "invalid_json": stats.invalid_json,
            "unknown_type": stats.unknown_type,
            "deserialize_failed": stats.deserialize_failed,
            "other": stats.other,
            "types": stats.unknown_types,
        },
        "ignored": stats.ignored,
        "ignored_types": stats.ignored_types,
    })
}

fn repeated_json(values: &[crate::analyze::RepeatedRead]) -> Vec<Value> {
    values
        .iter()
        .map(|item| {
            json!({
                "path": item.path,
                "count": item.count,
            })
        })
        .collect()
}

pub fn table(session_id: &str, report: &SessionReport, stats: ParseStats, compact: bool) -> String {
    let mut lines = vec![format!("SESSION {session_id}")];
    lines.push(format!(
        "User prompts: {} | Assistant messages: {} | Tool calls: {} | Span: {} | Active (est.): {} | Cache-read share: {}",
        format_number(report.user_prompts as u64),
        format_number(report.assistant_messages as u64),
        format_number(report.tool_calls as u64),
        duration(report.span_seconds),
        duration(report.active_seconds),
        cache_read_share(report).map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1}%"))
    ));
    lines.push(format!(
        "Resumed session: {}",
        match report.resumed_session {
            crate::analyze::ResumeStatus::Yes => "yes",
            crate::analyze::ResumeStatus::No => "no",
            crate::analyze::ResumeStatus::Unknown => "unknown",
        }
    ));
    section(&mut lines, "TOKENS", compact);
    lines.push(format!(
        "  input: {}",
        format_number(report.token_totals.input)
    ));
    lines.push(format!(
        "  output: {}",
        format_number(report.token_totals.output)
    ));
    lines.push(format!(
        "  cache read: {}",
        format_number(report.token_totals.cache_read)
    ));
    lines.push(format!(
        "  cache write: {}",
        format_number(report.token_totals.cache_write)
    ));

    section(&mut lines, "CONTEXT GROWTH", compact);
    for (label, value) in [
        ("first observed turn", report.context_growth[0]),
        ("middle", report.context_growth[1]),
        ("end", report.context_growth[2]),
    ] {
        lines.push(format!("  {label}: {}", display_option(value)));
    }
    if let Some(fact) = &report.context_fact {
        lines.push(format!("  {} of {} assistant calls had context >= 150,000 tokens ({:.1}% of cache-read tokens); first crossed at turn {}", fact.calls, fact.assistant_calls, fact.cache_read_percent, format_number(fact.first_turn as u64)));
    }
    lines.push(format!("  boundaries detected: {}", report.boundary_count));

    section(&mut lines, "TOP TOOL RESULTS", compact);
    if report.top_tool_results.is_empty() {
        lines.push(format!(
            "  size unavailable for {} blocks",
            report.unavailable_tool_result_sizes
        ));
    } else {
        for item in report.top_tool_results.iter().take(TOP_TOOL_RESULT_LIMIT) {
            lines.push(format!(
                "  {} | {} | turn {} | {} tokens (est.)",
                item.tool_name.as_deref().unwrap_or("unknown tool"),
                item.label.as_deref().unwrap_or("unknown target"),
                format_number(item.turn as u64),
                format_number(item.size)
            ));
        }
        if report.unavailable_tool_result_sizes > 0 {
            lines.push(format!(
                "  size unavailable for {} blocks",
                report.unavailable_tool_result_sizes
            ));
        }
    }
    section(&mut lines, "REPEATED READS", compact);
    if report.read_calls == 0 {
        lines.push(
            "  no Read tool calls in this session (file access via Bash is not tracked)".to_owned(),
        );
    } else if report.repeated_reads.is_empty() {
        lines.push(format!(
            "  none ({} Read calls, 0 repeated)",
            report.read_calls
        ));
    } else {
        repeated_lines(&mut lines, &report.repeated_reads, "reads");
    }
    section(&mut lines, "REPEATED EDITS", compact);
    if report.repeated_edits.is_empty() {
        lines.push(format!(
            "  none ({} Edit/Write calls, 0 repeated)",
            report.edit_write_calls
        ));
    } else {
        repeated_lines(&mut lines, &report.repeated_edits, "edits");
    }

    lines.push(format!(
        "unknown type: {} ({})",
        stats.unknown_type,
        type_breakdown(&stats.unknown_types)
    ));
    lines.push(format!(
        "Unparsed lines: {} (invalid JSON: {}, deserialize failed: {}, other: {})",
        format_number(stats.unparsed as u64),
        format_number(stats.invalid_json as u64),
        format_number(stats.deserialize_failed as u64),
        format_number(stats.other as u64)
    ));
    lines.push(format!(
        "Ignored lines: {} ({})",
        format_number(stats.ignored as u64),
        type_breakdown(&stats.ignored_types)
    ));
    lines.join("\n")
}

fn repeated_lines(lines: &mut Vec<String>, values: &[crate::analyze::RepeatedRead], noun: &str) {
    for item in values {
        lines.push(format!("  {} | {} {}", item.path, item.count, noun));
    }
}

fn type_breakdown(values: &std::collections::BTreeMap<String, usize>) -> String {
    let mut values: Vec<_> = values.iter().collect();
    values.sort_by(|(left_name, left_count), (right_name, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_name.cmp(right_name))
    });
    let shown = values
        .iter()
        .take(8)
        .map(|(name, count)| format!("{name}: {count}"))
        .collect::<Vec<_>>();
    let mut result = shown.join(", ");
    if values.len() > 8 {
        result.push_str(&format!(", +{} more", values.len() - 8));
    }
    result
}

fn section(lines: &mut Vec<String>, title: &str, compact: bool) {
    if !compact {
        lines.push(String::new());
    }
    lines.push(title.to_owned());
}

fn display_option(value: Option<u64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), format_number)
}
fn duration(seconds: Option<u64>) -> String {
    seconds.map_or_else(
        || "n/a".to_owned(),
        |seconds| format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
    )
}
fn cache_read_share(report: &SessionReport) -> Option<f64> {
    let input_side = report.token_totals.input
        + report.token_totals.cache_read
        + report.token_totals.cache_write;
    (input_side > 0).then(|| report.token_totals.cache_read as f64 * 100.0 / input_side as f64)
}
fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}
