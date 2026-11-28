# burnlog

burnlog reports token usage and activity in Claude Code and Codex sessions. It runs locally, reads logs without modifying them, and makes no network calls. Reports include metadata and token counts, not prompts or file contents.

## Install

Install the published release with Cargo:

```sh
cargo install burnlog
```

To update an existing installation, run `cargo install burnlog --force`. To install from a checkout, run `cargo install --path .`.

## Usage

Run burnlog from the project directory where you used the coding assistant. The default source is Claude Code:

```sh
burnlog report
```

To report the latest Codex session for the current project, select the Codex source:

```sh
burnlog report --source codex
```

For either source, pass a session ID to select a session, or use `--all` to report every session for the current project. Add `--json` for machine-readable output and `--compact` to remove extra whitespace:

```sh
burnlog report --source codex SESSION_ID
burnlog report --source codex --all
burnlog report --source codex --json
burnlog report --source claude --all --json --compact
```

Claude Code sessions are discovered under `~/.claude/projects`. Set `BURNLOG_CLAUDE_PROJECTS_DIR` to use a different projects directory. Codex sessions are discovered under `$CODEX_HOME/sessions`, or `~/.codex/sessions` when `CODEX_HOME` is unset. Codex archived sessions are not included.

## Report limits

- Unknown or malformed log lines are skipped and counted as unparsed. Usage coverage shows which token fields were present in the records burnlog counted.
- Active time and tool-result token sizes are estimates. Token-to-dollar estimates are not provided.
- Codex does not report cache-write usage. When a Codex log contains both cumulative snapshots and per-response usage records, burnlog uses the per-response records and warns that earlier usage may be missing.
- Reports can include tool names and paths. Review a report before sharing it.

See [Codex session reports](docs/codex-format.md) for format details.
