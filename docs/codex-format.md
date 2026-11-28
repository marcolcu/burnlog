# Codex session reports

Use `burnlog report --source codex` to report sessions for the current working directory. The default source remains Claude Code. Codex sessions are discovered under `$CODEX_HOME/sessions` (or `~/.codex/sessions` when `CODEX_HOME` is unset), using the working directory in the first session metadata record. Archived sessions are not included.

Codex usage records report cached input as part of input. Burnlog subtracts cached input from input and reports it separately. Cache-write usage is unavailable. For newer `token_usage_record` entries, response IDs are deduplicated; older `token_count` entries use the latest cumulative snapshot. If both forms occur, the report uses response records and flags that earlier usage may be omitted. Missing values are not assumed to be zero.

Assistant messages and tool calls use response items; user prompts use user-message events. Tool-result sizes, when text is available, are estimates. Other Codex event types and formats may not be represented. No prompt, command, tool result or file contents are printed.

Format references: [Codex token usage model](https://github.com/openai/codex/blob/main/codex-rs/tui/src/token_usage.rs), [per-response usage records](https://github.com/openai/codex/blob/main/codex-rs/core/src/state/session.rs), [rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs).
