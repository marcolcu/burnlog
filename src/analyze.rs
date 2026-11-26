//! Token usage analysis.

use std::collections::BTreeMap;

use crate::model::{Event, EventKind};

pub const INITIAL_CONTEXT_THRESHOLD: u64 = 100_000;
pub const CONTEXT_GROWTH_THRESHOLD: u64 = 150_000;
pub const TOOL_RESULT_THRESHOLD: u64 = 10_000;
pub const TOP_TOOL_RESULT_LIMIT: usize = 10;
pub const LABEL_MAX_CHARS: usize = 60;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPoint {
    pub turn: usize,
    pub size: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultSummary {
    pub tool_name: Option<String>,
    pub label: Option<String>,
    pub turn: usize,
    pub size: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatedRead {
    pub path: String,
    pub count: usize,
    pub summed_size: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub name: &'static str,
    pub fact: String,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionReport {
    pub token_totals: TokenTotals,
    pub context_growth: [Option<u64>; 3],
    pub initial_context_size: Option<u64>,
    pub top_tool_results: Vec<ToolResultSummary>,
    pub repeated_reads: Vec<RepeatedRead>,
    pub findings: Vec<Finding>,
}

pub fn analyze(events: &[Event]) -> SessionReport {
    let mut report = SessionReport::default();
    let mut contexts = BTreeMap::<usize, u64>::new();
    let mut reads = BTreeMap::<String, (usize, u64)>::new();
    let mut tools = Vec::new();
    for event in events {
        let usage = &event.usage;
        report.token_totals.input += usage.input_tokens.unwrap_or(0);
        report.token_totals.output += usage.output_tokens.unwrap_or(0);
        report.token_totals.cache_read += usage.cache_read_input_tokens.unwrap_or(0);
        report.token_totals.cache_write += usage.cache_creation_input_tokens.unwrap_or(0);
        if event.kind == EventKind::AssistantMessage {
            let size = usage.input_tokens.unwrap_or(0)
                + usage.cache_read_input_tokens.unwrap_or(0)
                + usage.cache_creation_input_tokens.unwrap_or(0);
            if usage.input_tokens.is_some()
                || usage.cache_read_input_tokens.is_some()
                || usage.cache_creation_input_tokens.is_some()
            {
                contexts
                    .entry(event.turn_index)
                    .and_modify(|current| *current = (*current).max(size))
                    .or_insert(size);
            }
        }
        if event.kind == EventKind::ToolResult {
            if let Some(size) = event.size_tokens {
                tools.push(ToolResultSummary {
                    tool_name: event.tool_name.clone(),
                    label: event.target_path.as_deref().map(short_label),
                    turn: event.turn_index,
                    size,
                });
            }
            if let Some(path) = &event.target_path {
                let value = reads.entry(path.clone()).or_default();
                value.0 += 1;
                value.1 += event.size_tokens.unwrap_or(0);
            }
        }
    }
    report.context_growth = context_snapshots(&contexts);
    report.initial_context_size = contexts.values().next().copied();
    tools.sort_by_key(|item| std::cmp::Reverse(item.size));
    tools.truncate(TOP_TOOL_RESULT_LIMIT);
    report.top_tool_results = tools;
    report.repeated_reads = reads
        .into_iter()
        .filter(|(_, (count, _))| *count > 1)
        .map(|(path, (count, summed_size))| RepeatedRead {
            path,
            count,
            summed_size,
        })
        .collect();
    if let Some(size) = report.initial_context_size
        && size > INITIAL_CONTEXT_THRESHOLD
    {
        report.findings.push(Finding {
            name: "initial_context_large",
            fact: format!("initial context measured {size} tokens"),
        });
    }
    let crossed = contexts
        .iter()
        .any(|(_, size)| *size > CONTEXT_GROWTH_THRESHOLD);
    if crossed && !events.iter().any(|event| event.kind == EventKind::Boundary) {
        report.findings.push(Finding {
            name: "context_threshold_without_boundary",
            fact: format!(
                "context exceeded {CONTEXT_GROWTH_THRESHOLD} tokens without a Boundary event"
            ),
        });
    }
    let large = report
        .top_tool_results
        .iter()
        .filter(|item| item.size > TOOL_RESULT_THRESHOLD)
        .count();
    if large > 0 {
        report.findings.push(Finding {
            name: "tool_result_large",
            fact: format!("{large} tool results exceeded {TOOL_RESULT_THRESHOLD} tokens"),
        });
    }
    report
}

fn context_snapshots(contexts: &BTreeMap<usize, u64>) -> [Option<u64>; 3] {
    let values: Vec<_> = contexts.values().copied().collect();
    if values.is_empty() {
        return [None, None, None];
    }
    [
        Some(values[0]),
        Some(values[(values.len() - 1) / 2]),
        values.last().copied(),
    ]
}

fn short_label(value: &str) -> String {
    let mut chars = value.chars();
    let label: String = chars.by_ref().take(LABEL_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{label}…")
    } else {
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Event, EventKind, TokenUsage};

    fn event(kind: EventKind, turn: usize) -> Event {
        Event {
            session_id: None,
            turn_index: turn,
            timestamp: None,
            kind,
            usage: TokenUsage::default(),
            tool_name: None,
            target_path: None,
            size_tokens: None,
            estimated: false,
        }
    }

    #[test]
    fn empty_and_no_usage_sessions_are_safe() {
        let empty = analyze(&[]);
        assert_eq!(empty.token_totals.input, 0);
        assert_eq!(empty.context_growth, [None, None, None]);
        let events = [event(EventKind::AssistantMessage, 1)];
        assert_eq!(analyze(&events).initial_context_size, None);
    }

    #[test]
    fn aggregates_usage_context_tools_and_reads() {
        let mut a = event(EventKind::AssistantMessage, 1);
        a.usage = TokenUsage {
            input_tokens: Some(120_000),
            output_tokens: Some(7),
            cache_read_input_tokens: Some(2),
            cache_creation_input_tokens: Some(3),
        };
        let mut result = event(EventKind::ToolResult, 1);
        result.tool_name = Some("Read".into());
        result.target_path = Some("/tmp/a".into());
        result.size_tokens = Some(20_001);
        let mut again = result.clone();
        again.turn_index = 2;
        let report = analyze(&[a, result, again]);
        assert_eq!(
            report.token_totals,
            TokenTotals {
                input: 120_000,
                output: 7,
                cache_read: 2,
                cache_write: 3
            }
        );
        assert_eq!(report.initial_context_size, Some(120_005));
        assert_eq!(report.repeated_reads[0].count, 2);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.name == "initial_context_large")
        );
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.name == "tool_result_large")
        );
    }

    #[test]
    fn context_threshold_rule_respects_boundary() {
        let mut a = event(EventKind::AssistantMessage, 1);
        a.usage.input_tokens = Some(1);
        let mut b = event(EventKind::AssistantMessage, 2);
        b.usage.input_tokens = Some(200_000);
        let mut boundary = event(EventKind::Boundary, 2);
        assert!(
            analyze(&[a.clone(), b.clone()])
                .findings
                .iter()
                .any(|f| f.name == "context_threshold_without_boundary")
        );
        boundary.turn_index = 1;
        assert!(
            !analyze(&[a, boundary, b])
                .findings
                .iter()
                .any(|f| f.name == "context_threshold_without_boundary")
        );
    }
}
