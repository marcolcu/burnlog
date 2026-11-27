# Changelog

## [0.1.1] - 2026-11-27

- Count tool calls per block while deduplicating usage by assistant `message.id`; estimate linked result sizes from text length.
- Report user prompts, assistant messages, tool calls, session span, estimated active time, and measured context threshold facts.
- Use privacy-safe tool labels, repeated Read/Edit targets, boundary counts, and typed ignored/unparsed breakdowns.

## [0.1.0] - 2026-11-26

Initial release.

- Analyze Claude Code session logs for token totals, context growth, tool results, repeated reads, and findings.
- Support table and JSON reports, with selection of one session or all sessions.
- Read logs locally without modifying them or making network calls.
