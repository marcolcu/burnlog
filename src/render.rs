//! Report output formatting.

use serde_json::{Value, json};

use crate::analyze::{ResumeStatus, SessionReport};
use crate::source::claude_code::ParseStats;

pub fn json_value(session_id: &str, report: &SessionReport, stats: ParseStats) -> Value {
    json!({
        "session_id": session_id,
        "turns": report.turns,
        "duration_seconds": report.duration_seconds,
        "resumed_session": resume_label(report.resumed_session),
        "cache_read_share_percent": cache_read_share(report),
        "token_totals": {
            "input": report.token_totals.input,
            "output": report.token_totals.output,
            "cache_read": report.token_totals.cache_read,
            "cache_write": report.token_totals.cache_write,
        },
        "context_growth": report.context_growth,
        "initial_context_size": report.initial_context_size,
        "top_tool_results": report.top_tool_results.iter().map(|item| json!({
            "tool_name": item.tool_name,
            "label": item.label,
            "turn": item.turn,
            "size": item.size,
        })).collect::<Vec<_>>(),
        "repeated_reads": report.repeated_reads.iter().map(|item| json!({
            "path": item.path,
            "count": item.count,
            "summed_size": item.summed_size,
        })).collect::<Vec<_>>(),
        "findings": report.findings.iter().map(|item| json!({
            "name": item.name,
            "fact": item.fact,
        })).collect::<Vec<_>>(),
        "unparsed": stats.unparsed,
        "unparsed_breakdown": {
            "invalid_json": stats.invalid_json,
            "unknown_type": stats.unknown_type,
            "deserialize_failed": stats.deserialize_failed,
            "other": stats.other,
        },
        "ignored": stats.ignored,
    })
}

pub fn table(session_id: &str, report: &SessionReport, stats: ParseStats, compact: bool) -> String {
    let mut lines = vec![format!("SESSION {session_id}")];
    let duration = report.duration_seconds.map_or_else(String::new, |seconds| {
        format!(" | Duration: {}s", format_number(seconds))
    });
    lines.push(format!(
        "Turns: {}{} | Cache-read share: {}",
        format_number(report.turns as u64),
        duration,
        cache_read_share(report).map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1}%"))
    ));
    lines.push(format!(
        "Resumed session: {}",
        resume_label(report.resumed_session)
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

    section(&mut lines, "TOP TOOL RESULTS", compact);
    if report.top_tool_results.is_empty() {
        lines.push(format!(
            "  {}",
            empty_tool_section(report.tool_blocks_parsed)
        ));
    } else {
        for item in &report.top_tool_results {
            lines.push(format!(
                "  {} | {} | turn {} | {} tokens",
                item.tool_name.as_deref().unwrap_or("unknown tool"),
                item.label.as_deref().unwrap_or("unknown target"),
                format_number(item.turn as u64),
                format_number(item.size)
            ));
        }
    }

    section(&mut lines, "REPEATED READS", compact);
    if report.repeated_reads.is_empty() {
        lines.push(format!(
            "  {}",
            empty_tool_section(report.tool_blocks_parsed)
        ));
    } else {
        for item in &report.repeated_reads {
            lines.push(format!(
                "  {} | {} reads | {} tokens",
                item.path,
                format_number(item.count as u64),
                format_number(item.summed_size)
            ));
        }
    }

    section(&mut lines, "FINDINGS", compact);
    if report.findings.is_empty() {
        lines.push("  none".to_owned());
    } else {
        for item in &report.findings {
            lines.push(format!(
                "  {}: {}",
                item.name,
                format_numbers_in_text(&item.fact)
            ));
        }
    }
    lines.push(format!(
        "Unparsed lines: {} (invalid JSON: {}, unknown type: {}, deserialize failed: {}, other: {})",
        format_number(stats.unparsed as u64),
        format_number(stats.invalid_json as u64),
        format_number(stats.unknown_type as u64),
        format_number(stats.deserialize_failed as u64),
        format_number(stats.other as u64)
    ));
    lines.push(format!(
        "Ignored lines: {}",
        format_number(stats.ignored as u64)
    ));
    lines.join("\n")
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

fn cache_read_share(report: &SessionReport) -> Option<f64> {
    let input_side = report.token_totals.input
        + report.token_totals.cache_read
        + report.token_totals.cache_write;
    (input_side > 0).then(|| report.token_totals.cache_read as f64 * 100.0 / input_side as f64)
}

fn resume_label(status: ResumeStatus) -> &'static str {
    match status {
        ResumeStatus::Yes => "yes",
        ResumeStatus::No => "no",
        ResumeStatus::Unknown => "unknown",
    }
}

fn empty_tool_section(tool_blocks: usize) -> String {
    if tool_blocks == 0 {
        "none found (0 tool blocks parsed)".to_owned()
    } else {
        format!(
            "none ({} tool blocks parsed, none qualified)",
            format_number(tool_blocks as u64)
        )
    }
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

fn format_numbers_in_text(value: &str) -> String {
    let mut formatted = String::new();
    let mut digits = String::new();
    for character in value.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_digit() {
            digits.push(character);
        } else {
            if !digits.is_empty() {
                if let Ok(number) = digits.parse::<u64>() {
                    formatted.push_str(&format_number(number));
                } else {
                    formatted.push_str(&digits);
                }
                digits.clear();
            }
            formatted.push(character);
        }
    }
    formatted.pop();
    formatted
}
