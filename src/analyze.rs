//! Token usage analysis.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

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
    pub turns: usize,
    pub duration_seconds: Option<u64>,
    pub resumed_session: ResumeStatus,
    pub tool_blocks_parsed: usize,
    pub token_totals: TokenTotals,
    pub context_growth: [Option<u64>; 3],
    pub initial_context_size: Option<u64>,
    pub top_tool_results: Vec<ToolResultSummary>,
    pub repeated_reads: Vec<RepeatedRead>,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResumeStatus {
    Yes,
    No,
    #[default]
    Unknown,
}

pub fn analyze(events: &[Event]) -> SessionReport {
    let mut report = SessionReport::default();
    let mut contexts = BTreeMap::<usize, u64>::new();
    let mut reads = BTreeMap::<String, (usize, u64)>::new();
    let mut tools = Vec::new();
    let mut turns = BTreeSet::new();
    let mut session_ids = BTreeSet::new();
    let mut timestamps = Vec::new();
    for event in events {
        if let Some(session_id) = &event.session_id {
            session_ids.insert(session_id);
        }
        if let Some(timestamp) = &event.timestamp
            && let Some(seconds) = timestamp_seconds(timestamp)
        {
            timestamps.push(seconds);
        }
        if matches!(event.kind, EventKind::ToolCall | EventKind::ToolResult) {
            report.tool_blocks_parsed += 1;
        }
        if matches!(
            event.kind,
            EventKind::AssistantMessage | EventKind::ToolCall
        ) && (event.usage.input_tokens.is_some()
            || event.usage.output_tokens.is_some()
            || event.usage.cache_read_input_tokens.is_some()
            || event.usage.cache_creation_input_tokens.is_some())
        {
            turns.insert(event.turn_index);
        }
        let usage = &event.usage;
        report.token_totals.input += usage.input_tokens.unwrap_or(0);
        report.token_totals.output += usage.output_tokens.unwrap_or(0);
        report.token_totals.cache_read += usage.cache_read_input_tokens.unwrap_or(0);
        report.token_totals.cache_write += usage.cache_creation_input_tokens.unwrap_or(0);
        if matches!(
            event.kind,
            EventKind::AssistantMessage | EventKind::ToolCall
        ) {
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
    report.turns = turns.len();
    report.resumed_session = if session_ids.len() > 1 {
        ResumeStatus::Yes
    } else {
        ResumeStatus::Unknown
    };
    if let (Some(first), Some(last)) = (timestamps.iter().min(), timestamps.iter().max()) {
        report.duration_seconds = Some((last - first) as u64);
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
    if report.resumed_session == ResumeStatus::No
        && let Some(size) = report.initial_context_size
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

fn timestamp_seconds(timestamp: &str) -> Option<i64> {
    let (date, time) = timestamp.split_once('T')?;
    let mut date = date.split('-');
    let year = date.next()?.parse::<i64>().ok()?;
    let month = date.next()?.parse::<i64>().ok()?;
    let day = date.next()?.parse::<i64>().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else {
        let index = time.char_indices().rev().find_map(|(index, ch)| {
            (index > 2 && matches!(ch, '+' | '-')).then_some((index, ch))
        })?;
        let (clock, offset) = time.split_at(index.0);
        let mut offset = offset[1..].split(':');
        let hours = offset.next()?.parse::<i64>().ok()?;
        let minutes = offset.next()?.parse::<i64>().ok()?;
        if offset.next().is_some() || hours > 23 || minutes > 59 {
            return None;
        }
        let seconds = hours * 3600 + minutes * 60;
        (clock, if index.1 == '+' { seconds } else { -seconds })
    };
    let clock = clock.split('.').next()?;
    let mut clock = clock.split(':');
    let hour = clock.next()?.parse::<i64>().ok()?;
    let minute = clock.next()?.parse::<i64>().ok()?;
    let second = clock.next()?.parse::<i64>().ok()?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(
        (era * 146_097 + day_of_era - 719_468) * 86_400 + hour * 3600 + minute * 60 + second
            - offset,
    )
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
                .all(|f| f.name != "initial_context_large")
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
