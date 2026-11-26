use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;
use thiserror::Error;

use crate::model::{Event, EventKind, TokenUsage};

const PROJECTS_DIR_ENV: &str = "BURNLOG_CLAUDE_PROJECTS_DIR";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParseStats {
    pub parsed: usize,
    pub unparsed: usize,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("failed to read Claude Code session log: {0}")]
    Io(#[from] io::Error),
}

/// Parse a Claude Code JSONL stream without retaining raw records.
pub fn parse<R: BufRead>(reader: R) -> Result<(Vec<Event>, ParseStats), ParseError> {
    let mut events = Vec::new();
    let mut stats = ParseStats::default();
    let mut seen_assistant_rows = HashSet::new();
    let mut turn_indices: HashMap<Option<String>, usize> = HashMap::new();
    let mut tool_names = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            stats.unparsed += 1;
            continue;
        };

        let Some(record_type) = record.get("type").and_then(Value::as_str) else {
            stats.unparsed += 1;
            continue;
        };
        if !matches!(record_type, "user" | "assistant" | "system") {
            stats.unparsed += 1;
            continue;
        }

        if record_type == "assistant" && !seen_assistant_rows.insert(line) {
            stats.parsed += 1;
            continue;
        }

        let session_id =
            string_field(&record, "sessionId").or_else(|| string_field(&record, "session_id"));
        let turn_index = *turn_indices.entry(session_id.clone()).or_default();
        let timestamp = string_field(&record, "timestamp");
        let mut row_events = match record_type {
            "assistant" => {
                remember_tool_names(&record, &mut tool_names);
                assistant_events(&record, session_id, turn_index, timestamp)
            }
            "user" => user_events(
                &record,
                session_id.clone(),
                turn_index,
                timestamp,
                &tool_names,
            ),
            "system"
                if record.get("subtype").and_then(Value::as_str) == Some("compact_boundary") =>
            {
                vec![event(
                    session_id,
                    turn_index,
                    timestamp,
                    EventKind::Boundary,
                )]
            }
            _ => Vec::new(),
        };

        if row_events.is_empty() {
            stats.unparsed += 1;
            continue;
        }
        if record_type == "user"
            && row_events
                .iter()
                .any(|item| item.kind == EventKind::UserPrompt)
            && let Some(index) = turn_indices.get_mut(&row_events[0].session_id)
        {
            *index += 1;
        }
        stats.parsed += 1;
        events.append(&mut row_events);
    }

    Ok((events, stats))
}

/// Resolve sessions from `BURNLOG_CLAUDE_PROJECTS_DIR` or the default Claude directory.
pub fn discover_sessions(cwd: &Path) -> io::Result<Vec<PathBuf>> {
    let root = match env::var_os(PROJECTS_DIR_ENV) {
        Some(path) => PathBuf::from(path),
        None => env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?
            .join(".claude/projects"),
    };
    discover_sessions_with_root(cwd, &root)
}

/// Find JSONL sessions for `cwd` under a Claude projects directory.
pub fn discover_sessions_with_root(cwd: &Path, projects_root: &Path) -> io::Result<Vec<PathBuf>> {
    let project_name = cwd.to_string_lossy().replace('/', "-");
    let project_dir = projects_root.join(project_name);
    let entries = match fs::read_dir(project_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        sessions.push((modified, path));
    }
    sessions.sort_by(|(left_time, left_path), (right_time, right_path)| {
        right_time
            .cmp(left_time)
            .then_with(|| left_path.cmp(right_path))
    });
    Ok(sessions.into_iter().map(|(_, path)| path).collect())
}

fn assistant_events(
    record: &Value,
    session_id: Option<String>,
    turn_index: usize,
    timestamp: Option<String>,
) -> Vec<Event> {
    let usage = token_usage(record);
    let content = record.pointer("/message/content").and_then(Value::as_array);
    let mut events = Vec::new();
    if let Some(content) = content {
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let mut item = event(
                        session_id.clone(),
                        turn_index,
                        timestamp.clone(),
                        EventKind::ToolCall,
                    );
                    item.tool_name = string_field(block, "name");
                    item.target_path = block
                        .pointer("/input/file_path")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    events.push(item);
                }
                Some("text") => events.push(event(
                    session_id.clone(),
                    turn_index,
                    timestamp.clone(),
                    EventKind::AssistantMessage,
                )),
                _ => {}
            }
        }
    }
    if events.is_empty() {
        events.push(event(
            session_id,
            turn_index,
            timestamp,
            EventKind::AssistantMessage,
        ));
    }
    events[0].usage = usage;
    events
}

fn user_events(
    record: &Value,
    session_id: Option<String>,
    turn_index: usize,
    timestamp: Option<String>,
    tool_names: &HashMap<String, String>,
) -> Vec<Event> {
    let content = record.pointer("/message/content");
    let blocks = content.and_then(Value::as_array);
    let results: Vec<_> = blocks
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        .collect();
    if results.is_empty() {
        return vec![event(
            session_id,
            turn_index + 1,
            timestamp,
            EventKind::UserPrompt,
        )];
    }
    results
        .iter()
        .map(|block| {
            let mut item = event(
                session_id.clone(),
                turn_index,
                timestamp.clone(),
                EventKind::ToolResult,
            );
            item.tool_name = block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .and_then(|tool_id| tool_names.get(tool_id))
                .cloned();
            item
        })
        .collect()
}

fn remember_tool_names(record: &Value, tool_names: &mut HashMap<String, String>) {
    let Some(blocks) = record.pointer("/message/content").and_then(Value::as_array) else {
        return;
    };
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("tool_use")
            && let (Some(id), Some(name)) = (
                block.get("id").and_then(Value::as_str),
                block.get("name").and_then(Value::as_str),
            )
        {
            tool_names.insert(id.to_owned(), name.to_owned());
        }
    }
}

fn token_usage(record: &Value) -> TokenUsage {
    let usage = record.pointer("/message/usage");
    TokenUsage {
        input_tokens: usage
            .and_then(|value| value.get("input_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .and_then(|value| value.get("output_tokens"))
            .and_then(Value::as_u64),
        cache_read_input_tokens: usage
            .and_then(|value| value.get("cache_read_input_tokens"))
            .and_then(Value::as_u64),
        cache_creation_input_tokens: usage
            .and_then(|value| value.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64),
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn event(
    session_id: Option<String>,
    turn_index: usize,
    timestamp: Option<String>,
    kind: EventKind,
) -> Event {
    Event {
        session_id,
        turn_index,
        timestamp,
        kind,
        usage: TokenUsage::default(),
        tool_name: None,
        target_path: None,
        size_tokens: None,
        estimated: false,
    }
}
