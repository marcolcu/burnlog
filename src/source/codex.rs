use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::model::{Event, EventKind, TokenUsage};
use crate::source::claude_code::ParseStats;

pub fn parse<R: BufRead>(reader: R) -> io::Result<(Vec<Event>, ParseStats)> {
    let mut events = Vec::new();
    let mut stats = ParseStats {
        cache_write_unavailable: true,
        ..ParseStats::default()
    };
    let mut session_id = None;
    let mut turn = 0;
    let mut responses = HashSet::new();
    let mut user_items = HashSet::new();
    let mut legacy_user_messages = false;
    let mut tools = HashMap::new();
    let mut usage_events = Vec::new();
    let mut snapshot = None;
    for line in reader.lines() {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            stats.invalid_json += 1;
            continue;
        };
        let Some(kind) = record.get("type").and_then(Value::as_str) else {
            stats.other += 1;
            continue;
        };
        let payload = &record["payload"];
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut item = Event {
            session_id: session_id.clone(),
            turn_index: turn,
            timestamp,
            assistant_message_id: None,
            kind: EventKind::Usage,
            usage: TokenUsage::default(),
            tool_name: None,
            tool_label: None,
            target_path: None,
            size_tokens: None,
            estimated: false,
        };
        match kind {
            "session_meta" => {
                session_id = payload.get("id").and_then(Value::as_str).map(str::to_owned);
                stats.session_id_count = usize::from(session_id.is_some());
            }
            "event_msg" if payload["type"] == "user_message" => {
                legacy_user_messages = true;
                turn += 1;
                item.turn_index = turn;
                item.kind = EventKind::UserPrompt;
                events.push(item);
            }
            "event_msg"
                if payload["type"] == "item_completed"
                    && payload["item"]["type"] == "UserMessage" =>
            {
                if let Some(id) = payload.pointer("/item/id").and_then(Value::as_str) {
                    if user_items.insert(id.to_owned()) {
                        turn += 1;
                        item.turn_index = turn;
                        item.kind = EventKind::UserPrompt;
                        events.push(item);
                    }
                } else {
                    stats.deserialize_failed += 1;
                    continue;
                }
            }
            "event_msg" if payload["type"] == "token_count" => {
                if let Some(size) = payload
                    .pointer("/info/last_token_usage/total_tokens")
                    .and_then(Value::as_u64)
                {
                    let mut context = item.clone();
                    context.kind = EventKind::Context;
                    context.size_tokens = Some(size);
                    events.push(context);
                }
                if let Some(value) = payload
                    .pointer("/info/total_token_usage")
                    .filter(|value| value.is_object())
                {
                    item.usage = usage(value);
                    snapshot = Some(item);
                }
            }
            "token_usage_record" => {
                if let (Some(id), Some(value)) = (
                    payload.get("response_id").and_then(Value::as_str),
                    payload.get("usage").filter(|value| value.is_object()),
                ) {
                    if responses.insert(id.to_owned()) {
                        item.usage = usage(value);
                        usage_events.push(item);
                    }
                } else {
                    stats.deserialize_failed += 1;
                    continue;
                }
            }
            "response_item" if payload["type"] == "message" && payload["role"] == "assistant" => {
                item.kind = EventKind::AssistantMessage;
                events.push(item);
            }
            "response_item"
                if matches!(
                    payload["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                ) =>
            {
                item.kind = EventKind::ToolCall;
                item.tool_name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                item.tool_label = item
                    .tool_name
                    .as_deref()
                    .and_then(|name| codex_tool_label(name, payload));
                if let (Some(id), Some(name)) = (
                    payload.get("call_id").and_then(Value::as_str),
                    &item.tool_name,
                ) {
                    tools.insert(id.to_owned(), (name.clone(), item.tool_label.clone()));
                }
                events.push(item);
            }
            "response_item"
                if matches!(
                    payload["type"].as_str(),
                    Some("function_call_output" | "custom_tool_call_output")
                ) =>
            {
                item.kind = EventKind::ToolResult;
                if let Some((name, label)) = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .and_then(|id| tools.get(id))
                {
                    item.tool_name = Some(name.clone());
                    item.tool_label = label.clone();
                }
                item.size_tokens = payload.get("output").and_then(output_size);
                item.estimated = item.size_tokens.is_some();
                events.push(item);
            }
            "compacted" => {
                item.kind = EventKind::Boundary;
                events.push(item);
            }
            "turn_context" => {
                stats.ignored += 1;
                continue;
            }
            "response_item" | "event_msg" => {
                let subtype = payload.get("type").and_then(Value::as_str);
                if matches!(
                    (kind, subtype),
                    ("response_item", Some("reasoning" | "message"))
                        | (
                            "event_msg",
                            Some(
                                "agent_message"
                                    | "task_started"
                                    | "task_complete"
                                    | "turn_started"
                                    | "turn_complete"
                                    | "item_started"
                                    | "item_completed"
                                    | "exec_command_begin"
                                    | "exec_command_end"
                                    | "agent_reasoning"
                                    | "agent_reasoning_raw_content"
                                    | "turn_aborted"
                                    | "context_compacted"
                            )
                        )
                ) {
                    stats.ignored += 1;
                    *stats.ignored_types.entry(kind.to_owned()).or_default() += 1;
                } else {
                    stats.unknown_type += 1;
                    *stats.unknown_types.entry(kind.to_owned()).or_default() += 1;
                }
                continue;
            }
            _ => {
                stats.unknown_type += 1;
                *stats.unknown_types.entry(kind.to_owned()).or_default() += 1;
                continue;
            }
        }
        stats.parsed += 1;
    }
    stats.prompt_count_uncertain = (legacy_user_messages && !user_items.is_empty()) || turn == 0;
    stats.usage_mixed_sources = !usage_events.is_empty() && snapshot.is_some();
    stats.usage_coverage_basis = if !usage_events.is_empty() {
        Some("response_records")
    } else if snapshot.is_some() {
        Some("cumulative_snapshot")
    } else {
        None
    };
    if usage_events.is_empty()
        && let Some(item) = snapshot
    {
        usage_events.push(item);
    }
    for item in &usage_events {
        stats.usage_messages += 1;
        stats.usage_input += usize::from(item.usage.input_tokens.is_some());
        stats.usage_output += usize::from(item.usage.output_tokens.is_some());
        stats.usage_cache_read += usize::from(item.usage.cache_read_input_tokens.is_some());
    }
    events.extend(usage_events);
    stats.unparsed =
        stats.invalid_json + stats.unknown_type + stats.deserialize_failed + stats.other;
    Ok((events, stats))
}

fn codex_tool_label(name: &str, payload: &Value) -> Option<String> {
    match name {
        "exec" => Some("tool execution".to_owned()),
        "exec_command" | "shell_command" | "shell" => {
            let arguments = payload
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|value| serde_json::from_str::<Value>(value).ok());
            let command = arguments
                .as_ref()
                .and_then(|value| value.get("cmd").or_else(|| value.get("command")))
                .and_then(Value::as_str)
                .and_then(|value| value.split_whitespace().next());
            let label = command
                .filter(|command| {
                    matches!(
                        *command,
                        "rg" | "cat"
                            | "sed"
                            | "head"
                            | "tail"
                            | "git"
                            | "cargo"
                            | "ls"
                            | "find"
                            | "pwd"
                            | "wc"
                            | "sort"
                            | "diff"
                    )
                })
                .unwrap_or("shell command");
            Some(label.to_owned())
        }
        _ => None,
    }
}

fn output_size(output: &Value) -> Option<u64> {
    let chars = if let Some(text) = output.as_str() {
        text.chars().count()
    } else {
        output
            .as_array()?
            .iter()
            .map(|item| {
                (item.get("type")?.as_str()? == "input_text").then_some(())?;
                Some(item.get("text")?.as_str()?.chars().count())
            })
            .sum::<Option<usize>>()?
    };
    Some((chars / 4) as u64)
}

fn usage(value: &Value) -> TokenUsage {
    let input = value.get("input_tokens").and_then(Value::as_u64);
    let cached = value.get("cached_input_tokens").and_then(Value::as_u64);
    TokenUsage {
        input_tokens: input
            .zip(cached)
            .and_then(|(input, cached)| input.checked_sub(cached)),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: None,
    }
}

pub fn discover_sessions(cwd: &Path) -> io::Result<Vec<PathBuf>> {
    let root = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?
        .join("sessions");
    discover_sessions_with_root(cwd, &root)
}

pub fn discover_sessions_with_root(cwd: &Path, root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut dirs = vec![root.to_path_buf()];
    let mut sessions = Vec::new();
    while let Some(dir) = dirs.pop() {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                dirs.push(entry.path());
                continue;
            }
            let path = entry.path();
            if !ty.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let mut reader = BufReader::new(File::open(&path)?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let matches = serde_json::from_str::<Value>(&line)
                .ok()
                .is_some_and(|value| {
                    value["type"] == "session_meta"
                        && value
                            .pointer("/payload/cwd")
                            .and_then(Value::as_str)
                            .is_some_and(|path| {
                                Path::new(path) == cwd
                                    || fs::canonicalize(path).ok().as_deref() == Some(cwd)
                            })
                });
            if matches {
                sessions.push((
                    entry
                        .metadata()?
                        .modified()
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                    path,
                ));
            }
        }
    }
    sessions.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(sessions.into_iter().map(|(_, path)| path).collect())
}
