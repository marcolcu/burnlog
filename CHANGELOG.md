# Changelog

## [0.1.1] - 2026-11-27

- Parse nested tool results and Read targets, and deduplicate repeated assistant message IDs.
- Report unparsed reasons, ignored metadata, resume uncertainty, turn context, duration, and cache-read share.
- Format table numbers with separators and distinguish empty tool sections.

## [0.1.0] - 2026-11-26

Initial release.

- Analyze Claude Code session logs for token totals, context growth, tool results, repeated reads, and findings.
- Support table and JSON reports, with selection of one session or all sessions.
- Read logs locally without modifying them or making network calls.
