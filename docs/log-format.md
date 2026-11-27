# Claude Code session log format

Claude Code stores session logs as JSON Lines under
`~/.claude/projects/<project>/*.jsonl`. Each line is a JSON object. Records in
one file are heterogeneous: message records sit beside metadata, snapshots,
attachments, API request records, and other event types. Read logs as a stream;
they can be large and may contain private code, prompts, and secrets.

## Evidence sampled

Inspected only JSON structure, field names, types, and counts from three local
sessions. No message text, tool input values, file contents, or log paths were
used in this document.

| Sample | File size | Lines | Observed record types |
| --- | ---: | ---: | --- |
| Small | 45,601 bytes | 17 | `attachment`, `cost-state`, `file-history-snapshot`, `last-prompt`, `mode`, `permission-mode`, `user` |
| Medium | 1,428,360 bytes | 361 | 15 types, including `assistant`, `user`, `system`, `api-request`, `attachment` |
| Large | 414,227,697 bytes | 65,846 | 16 types, including `assistant`, `user`, `system`, `queue-operation`, `file-history-delta` |

The small file had no assistant rows. The medium file had 45 assistant rows and
the large file had 19,384. The large file alone contained 124
`system/compact_boundary` rows. Field sets and event types differ between
sessions; a parser must tolerate missing and additional fields.

## Fields relevant to a token report

Paths are relative to one JSONL record. The path names and structures below were
observed in the medium and/or large sample unless marked otherwise.

| Use | JSON path | Notes |
| --- | --- | --- |
| Event kind | `$.type` | Includes `assistant`, `user`, `system`, and non-message metadata event types. |
| Session ID | `$.sessionId`; sometimes `$.session_id` | Both spellings appeared in the sampled records. |
| Timestamp | `$.timestamp` | Present on message-like and some metadata rows; not every row. |
| Log format/client version | `$.version` | Not on every record; values differed across sessions and within the large sample. |
| Assistant message ID | `$.message.id` | Present in the nested assistant message. Repeated IDs occurred. |
| Record linkage | `$.uuid`, `$.parentUuid` | Parent references linked to UUIDs in the sample. `$.logicalParentUuid` also appeared on a compact boundary record. |
| Input tokens | `$.message.usage.input_tokens` | Found on every sampled assistant row. |
| Output tokens | `$.message.usage.output_tokens` | Found on every sampled assistant row. |
| Cache-read tokens | `$.message.usage.cache_read_input_tokens` | Found on every sampled assistant row. |
| Cache-write tokens | `$.message.usage.cache_creation_input_tokens` | Found on every sampled assistant row. |
| Tool call | `$.message.content[]` where `type == "tool_use"` | Block has `id`, `name`, and `input`; tool names observed include `Bash` and `Read`. |
| Tool result | `$.message.content[]` where `type == "tool_result"` | Result has `tool_use_id`, `content`, and `is_error` (the latter was not present on every result). Match `tool_use_id` to the call's `id`. |
| Compaction boundary | `$.type == "system"` and `$.subtype == "compact_boundary"` | Large sample also had a `compactMetadata` field on these records. |
| Local command event | `$.type == "system"` and `$.subtype == "local_command"` | One such event occurred in the medium sample. Its content was not inspected. |

Usage is attached per assistant record, nested under its `message`. It is not a
session-level total. Repeated assistant rows mean summing every row can count
the same message usage more than once. Do not treat missing usage as zero
without recording that the source field was absent.

## Tools and file targets

Calls appear as `tool_use` blocks inside an assistant message's `content`
array. Results appear as `tool_result` blocks in a user message's `content`
array. Result `content` was a string in the medium sample and a string or array
in the large sample. No consistent byte-size or token-size field for tool input
or result was found. A consumer can measure returned text length, but that is a
derived size and must be labeled estimated; it does not represent tokens used
by the model.

Tool arguments are nested under `tool_use.input`. The sampled `Read` tool used
the `file_path` argument. `Bash` used `command`; a shell command may mention a
path, but the log does not expose a normalized target-path field for it. No
`Grep` call occurred in the selected samples, so its target argument name is
unconfirmed here. Do not print argument contents: they can include source code,
prompts, paths, or secrets.

## Duplicate and boundary behavior

The large sample contained exact duplicate assistant JSONL rows and repeated
assistant message IDs; the medium sample had repeated assistant message IDs but
no exact duplicate assistant rows. A row count is therefore not a safe proxy
for a unique-message count or usage total. A stable deduplication rule needs to
distinguish replayed copies from legitimate multiple assistant records with
the same message ID before summing usage.

`compact_boundary` is an explicit compaction marker in the sampled large
session. `/clear` was not confirmed as a distinct structural event. A
`local_command` event was seen, but its content was intentionally not read, so
the sample does not establish whether command text, a new session file, or
another field marks `/clear`.

## Unknowns / must be estimated

- Whether input-token accounting should add cached tokens to
  `input_tokens`, or report the four usage values separately. The fields are
  distinct in logs; their aggregation semantics were not validated here.
- How to distinguish streamed/replayed assistant records from legitimate
  assistant messages sharing `message.id`; exact duplicate rows exist.
- The `Grep` input field name and whether other tools expose a normalized file
  target. It was not observed in the selected sessions.
- Tool-result size in bytes/tokens. No consistent direct size field was found;
  any length calculated from `content` is estimated.
- The structural marker for `/clear` and whether it starts a new session file.
- Whether `version` tracks the CLI, a log schema, or both, and how fields map
  across versions. Sampled values included `2.1.304`, `2.1.308`, and
  `2.1.323`; no separate schema-version field was identified.
- Whether `timestamp` is always an ISO timestamp and how absent timestamps
  should be handled.
- Whether `$.message.usage` is always per API response when an assistant turn
  contains multiple assistant rows; broader sampling is needed.

## Synthetic fixtures

Three small invented examples in `tests/fixtures/` cover a user/assistant
exchange with usage, a tool call/result pair, and a compaction boundary. Their
IDs, text, and paths are synthetic; they are structural examples, not real
logs.

## Parser handling

The sampled `attachment`, `cost-state`, `file-history-snapshot`,
`file-history-delta`, `last-prompt`, `mode`, `permission-mode`, `api-request`,
and `queue-operation` records carry no assistant usage or tool blocks and can
be ignored by a token report. Other unrecognized record types are unexpected.
A known `user` or `assistant` record without a message object is malformed.
Only `system/compact_boundary` is currently used; other system subtypes are
unexpected because their usage and tool behavior are not established here.
Multiple distinct session IDs in one file are direct evidence of a resumed
session. The sampled format provides no reliable evidence that a single-ID
file was not resumed, so its resume status is unknown.

Tool calls and results are nested in their respective `message.content[]`
arrays. A result's `tool_use_id` links it to a call's `id`; that link also
provides the call's tool name and `Read` path. Repeated assistant `message.id`
values identify a replay for usage deduplication.
