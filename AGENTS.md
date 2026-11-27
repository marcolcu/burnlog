PROJECT: burnlog, local read-only CLI that reports where AI coding-session
tokens go. v0.1.0 scope: Claude Code logs only, `report` command, table and
--json output. Out of scope: network, dashboards, dollar cost, Codex, AI-based
suggestions, modifying logs.

COMMANDS: build `cargo build`; test `cargo test`; lint
`cargo clippy --all-targets -- -D warnings`; format `cargo fmt`.
All three of test/lint/fmt-check must pass before any commit.

ARCHITECTURE: raw log line -> source::* parser -> model::Event (internal) ->
analyze::* -> render::*. analyze and render never touch raw log formats.
All logic lives in the lib crate; main.rs only parses args and prints.

RULES:
- Parsers stream line by line (BufRead); never load a whole file into memory.
- Parsers are tolerant: unknown or malformed lines are skipped and counted as
  `unparsed`, never panic or abort. Use Option and #[serde(default)].
- Errors: thiserror for library error types, anyhow only in main.rs.
  No unwrap/expect outside tests.
- Never make network calls. Never write to or modify log files.
- Never print raw prompt or file contents from logs; only metadata, sizes,
  counts, tool names, paths.
- Test fixtures in tests/fixtures/ are anonymized synthetic logs only.
  Never commit real logs.
- Every feature: write the test first, then the code. Prefer small pure
  functions over traits; add a trait only when a second implementation exists.
- Keep dependencies minimal; ask before adding a new crate.
- Commits: conventional commits, one logical change each.
- Estimated values (not read from logs) must be labeled "estimated" in output.
- Tool blocks live in `message.content[]`; link results by `tool_use_id` and deduplicate assistant usage by `message.id`.
- Assistant log lines may hold one content block each while repeating `message.id`; count usage once per ID and count every tool-use block.
- Tool-result sizes are estimated from returned text length and must be labeled estimated.
