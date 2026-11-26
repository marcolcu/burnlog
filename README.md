# burnlog

AI coding sessions can use many tokens on context, tool results, and repeated file reads, but the logs make those costs hard to see. burnlog summarizes Claude Code session token usage and related activity in a local report.

burnlog runs locally, reads Claude Code logs without changing them, and makes no network calls. It reports metadata and token counts; it never prints prompts or file contents.

## Install

Install the published release with Cargo:

```sh
cargo install burnlog
```

Or build from a checkout:

```sh
cargo install --path .
```

## Usage

Run a report from a project directory that has Claude Code session logs:

```sh
burnlog report
```

By default, burnlog finds the most recent session for the current directory. Use `--all` to report every session, pass a session ID to select one, or add `--json` for machine-readable output.

This output was produced by running `burnlog report` against the synthetic fixture `tests/fixtures/claude-session-basic.jsonl`:

```text
SESSION fixture-session-basic

TOKENS
  input: 12
  output: 5
  cache read: 3
  cache write: 2

CONTEXT GROWTH
  start: 17
  middle: 17
  end: 17

TOP TOOL RESULTS
  none

REPEATED READS
  none

FINDINGS
  none
Unparsed lines: 0
```

## Limits

- v0.1.0 reads Claude Code session logs only.
- Reports cover token totals, context growth, tool results, repeated reads, and findings. Token-to-dollar estimates are not provided.
- Session logs are discovered for the current project under `~/.claude/projects`. Set `BURNLOG_CLAUDE_PROJECTS_DIR` to use another Claude projects directory.
- Unknown or malformed log lines are skipped and counted as unparsed.

## Roadmap

- Add more report options based on user feedback.
- Consider support for other coding assistants after the Claude Code format is stable.
