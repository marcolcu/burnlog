//! Report output formatting.

use serde_json::{Value, json};

use crate::analyze::SessionReport;

pub fn json_value(session_id: &str, report: &SessionReport, unparsed: usize) -> Value {
    json!({
        "session_id": session_id,
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
        "unparsed": unparsed,
    })
}

pub fn table(session_id: &str, report: &SessionReport, unparsed: usize, compact: bool) -> String {
    let mut lines = vec![format!("SESSION {session_id}")];
    section(&mut lines, "TOKENS", compact);
    lines.push(format!("  input: {}", report.token_totals.input));
    lines.push(format!("  output: {}", report.token_totals.output));
    lines.push(format!("  cache read: {}", report.token_totals.cache_read));
    lines.push(format!(
        "  cache write: {}",
        report.token_totals.cache_write
    ));

    section(&mut lines, "CONTEXT GROWTH", compact);
    for (label, value) in [
        ("start", report.context_growth[0]),
        ("middle", report.context_growth[1]),
        ("end", report.context_growth[2]),
    ] {
        lines.push(format!("  {label}: {}", display_option(value)));
    }

    section(&mut lines, "TOP TOOL RESULTS", compact);
    if report.top_tool_results.is_empty() {
        lines.push("  none".to_owned());
    } else {
        for item in &report.top_tool_results {
            lines.push(format!(
                "  {} | {} | turn {} | {} tokens",
                item.tool_name.as_deref().unwrap_or("unknown tool"),
                item.label.as_deref().unwrap_or("unknown target"),
                item.turn,
                item.size
            ));
        }
    }

    section(&mut lines, "REPEATED READS", compact);
    if report.repeated_reads.is_empty() {
        lines.push("  none".to_owned());
    } else {
        for item in &report.repeated_reads {
            lines.push(format!(
                "  {} | {} reads | {} tokens",
                item.path, item.count, item.summed_size
            ));
        }
    }

    section(&mut lines, "FINDINGS", compact);
    if report.findings.is_empty() {
        lines.push("  none".to_owned());
    } else {
        for item in &report.findings {
            lines.push(format!("  {}: {}", item.name, item.fact));
        }
    }
    lines.push(format!("Unparsed lines: {unparsed}"));
    lines.join("\n")
}

fn section(lines: &mut Vec<String>, title: &str, compact: bool) {
    if !compact {
        lines.push(String::new());
    }
    lines.push(title.to_owned());
}

fn display_option(value: Option<u64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| value.to_string())
}
