# Changelog

## [0.1.1] - 2026-11-27

- Parse tool blocks nested in `message.content`, link tool results by `tool_use_id`, and deduplicate assistant usage by `message.id`.
- Break down unparsed lines by reason and report resume detection.
- Label context growth as "first observed turn" and empty sections as "none found (0 tool blocks parsed)".
- Format numbers with separators and add turns, duration, and cache-read share to the report header.

## [0.1.0] - 2026-11-26

Initial release.

- Analyze Claude Code session logs for token totals, context growth, tool results, repeated reads, and findings.
- Support table and JSON reports, with selection of one session or all sessions.
- Read logs locally without modifying them or making network calls.
