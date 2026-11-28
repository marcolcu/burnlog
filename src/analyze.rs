//! Token usage analysis.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::model::{Event, EventKind};

pub const CONTEXT_GROWTH_THRESHOLD: u64 = 150_000;
pub const TOP_TOOL_RESULT_LIMIT: usize = 10;
pub const ACTIVE_GAP_SECS: i64 = 300;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}
#[derive(Clone, Debug, PartialEq)]
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
    pub estimated: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatedRead {
    pub path: String,
    pub count: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ContextFact {
    pub calls: usize,
    pub assistant_calls: usize,
    pub cache_read_percent: f64,
    pub first_turn: usize,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionReport {
    pub user_prompts: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub span_seconds: Option<u64>,
    pub active_seconds: Option<u64>,
    pub resumed_session: ResumeStatus,
    pub unavailable_tool_result_sizes: usize,
    pub token_totals: TokenTotals,
    pub context_growth: [Option<u64>; 3],
    pub initial_context_size: Option<u64>,
    pub top_tool_results: Vec<ToolResultSummary>,
    pub repeated_reads: Vec<RepeatedRead>,
    pub repeated_edits: Vec<RepeatedRead>,
    pub read_calls: usize,
    pub edit_write_calls: usize,
    pub context_fact: Option<ContextFact>,
    pub boundary_count: usize,
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
    let mut reads = BTreeMap::<String, usize>::new();
    let mut edits = BTreeMap::<String, usize>::new();
    let mut tools = Vec::new();
    let mut assistant_ids = BTreeSet::new();
    let mut assistant_calls = Vec::new();
    let mut session_ids = BTreeSet::new();
    let mut timestamps = Vec::new();
    let cwd = std::env::current_dir().ok();
    for event in events {
        if let Some(session_id) = &event.session_id {
            session_ids.insert(session_id);
        }
        if let Some(timestamp) = &event.timestamp
            && let Some(seconds) = timestamp_seconds(timestamp)
        {
            timestamps.push(seconds);
        }
        if event.kind == EventKind::ToolCall {
            report.tool_calls += 1;
        }
        if event.kind == EventKind::UserPrompt {
            report.user_prompts += 1;
        }
        if event.kind == EventKind::Boundary {
            report.boundary_count += 1;
        }
        if event.kind == EventKind::Context
            && let Some(size) = event.size_tokens
        {
            contexts.insert(event.turn_index, size);
        }
        if matches!(
            event.kind,
            EventKind::AssistantMessage | EventKind::ToolCall
        ) {
            if let Some(id) = &event.assistant_message_id {
                assistant_ids.insert(id.clone());
            } else if event.kind == EventKind::AssistantMessage {
                report.assistant_messages += 1;
            }
        }
        if matches!(
            event.kind,
            EventKind::AssistantMessage | EventKind::ToolCall
        ) && (event.usage.input_tokens.is_some()
            || event.usage.cache_read_input_tokens.is_some()
            || event.usage.cache_creation_input_tokens.is_some())
            && (event.usage.input_tokens.is_some() || event.usage.cache_read_input_tokens.is_some())
        {
            let context = event.usage.input_tokens.unwrap_or(0)
                + event.usage.cache_read_input_tokens.unwrap_or(0)
                + event.usage.cache_creation_input_tokens.unwrap_or(0);
            assistant_calls.push((
                event.assistant_message_id.clone(),
                event.turn_index,
                context,
                event.usage.cache_read_input_tokens.unwrap_or(0),
            ));
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
                    label: event.tool_label.clone().or_else(|| {
                        event
                            .target_path
                            .as_deref()
                            .map(|path| safe_path_label(path, cwd.as_deref()))
                    }),
                    turn: event.turn_index,
                    size,
                    estimated: event.estimated,
                });
            } else {
                report.unavailable_tool_result_sizes += 1;
            }
        }
        if event.kind == EventKind::ToolCall {
            if event.tool_name.as_deref() == Some("Read") {
                report.read_calls += 1;
                if let Some(path) = &event.target_path {
                    *reads
                        .entry(safe_path_label(path, cwd.as_deref()))
                        .or_default() += 1;
                }
            } else if matches!(event.tool_name.as_deref(), Some("Edit" | "Write")) {
                report.edit_write_calls += 1;
                if let Some(path) = &event.target_path {
                    *edits
                        .entry(safe_path_label(path, cwd.as_deref()))
                        .or_default() += 1;
                }
            }
        }
    }
    report.assistant_messages += assistant_ids.len();
    report.resumed_session = if session_ids.len() > 1 {
        ResumeStatus::Yes
    } else {
        ResumeStatus::Unknown
    };
    if let (Some(first), Some(last)) = (timestamps.iter().min(), timestamps.iter().max()) {
        report.span_seconds = Some((last - first) as u64);
    }
    timestamps.sort_unstable();
    report.active_seconds = Some(
        timestamps
            .windows(2)
            .filter_map(|pair| {
                let gap = pair[1] - pair[0];
                (gap > 0 && gap < ACTIVE_GAP_SECS).then_some(gap as u64)
            })
            .sum(),
    );
    report.context_growth = context_snapshots(&contexts);
    report.initial_context_size = contexts.values().next().copied();
    tools.sort_by_key(|item| std::cmp::Reverse(item.size));
    tools.truncate(TOP_TOOL_RESULT_LIMIT);
    report.top_tool_results = tools;
    report.repeated_reads = repeated(reads);
    report.repeated_edits = repeated(edits);
    let unique_calls: BTreeMap<_, _> = assistant_calls
        .into_iter()
        .map(|(id, turn, context, cache)| {
            (
                (
                    id.unwrap_or_else(|| format!("turn-{turn}-{context}-{cache}")),
                    turn,
                ),
                (context, cache),
            )
        })
        .collect();
    let crossed: Vec<_> = unique_calls
        .values()
        .filter(|(context, _)| *context >= CONTEXT_GROWTH_THRESHOLD)
        .collect();
    if let Some(first_turn) = unique_calls
        .iter()
        .filter(|(_, (context, _))| *context >= CONTEXT_GROWTH_THRESHOLD)
        .map(|((_, turn), _)| *turn)
        .min()
    {
        let cache_total: u64 = unique_calls.values().map(|(_, cache)| cache).sum();
        let cache_crossed: u64 = crossed.iter().map(|(_, cache)| *cache).sum();
        report.context_fact = Some(ContextFact {
            calls: crossed.len(),
            assistant_calls: unique_calls.len(),
            cache_read_percent: if cache_total == 0 {
                0.0
            } else {
                cache_crossed as f64 * 100.0 / cache_total as f64
            },
            first_turn,
        });
    }
    report
}

fn repeated(values: BTreeMap<String, usize>) -> Vec<RepeatedRead> {
    values
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(path, count)| RepeatedRead { path, count })
        .collect()
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

fn safe_path_label(value: &str, cwd: Option<&Path>) -> String {
    let path = PathBuf::from(value);
    if let Some(relative) = cwd.and_then(|cwd| path.strip_prefix(cwd).ok()) {
        return relative.to_string_lossy().into_owned();
    }
    path.components()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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
            assistant_message_id: None,
            kind,
            usage: TokenUsage::default(),
            tool_name: None,
            tool_label: None,
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
        let mut result = event(EventKind::ToolCall, 1);
        result.tool_name = Some("Read".into());
        result.target_path = Some("/tmp/a".into());
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
        assert_eq!(report.tool_calls, 2);
    }

    #[test]
    fn context_threshold_rule_respects_boundary() {
        let mut a = event(EventKind::AssistantMessage, 1);
        a.usage.input_tokens = Some(1);
        let mut b = event(EventKind::AssistantMessage, 2);
        b.usage.input_tokens = Some(200_000);
        let mut boundary = event(EventKind::Boundary, 2);
        assert!(analyze(&[a.clone(), b.clone()]).context_fact.is_some());
        boundary.turn_index = 1;
        assert_eq!(analyze(&[a, boundary, b]).boundary_count, 1);
    }

    #[test]
    fn active_time_excludes_gaps_of_five_minutes_or_more_and_results_are_limited() {
        let mut events = Vec::new();
        for timestamp in [
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:04:59Z",
            "2026-01-01T00:09:59Z",
        ] {
            let mut item = event(EventKind::UserPrompt, 1);
            item.timestamp = Some(timestamp.to_owned());
            events.push(item);
        }
        for size in 1..=12 {
            let mut item = event(EventKind::ToolResult, 1);
            item.size_tokens = Some(size);
            item.estimated = true;
            events.push(item);
        }
        let report = analyze(&events);
        assert_eq!(report.span_seconds, Some(599));
        assert_eq!(report.active_seconds, Some(299));
        assert_eq!(report.top_tool_results.len(), TOP_TOOL_RESULT_LIMIT);
        assert_eq!(report.top_tool_results[0].size, 12);
    }
}
