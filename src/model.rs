//! Core session and token data models.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub session_id: Option<String>,
    pub turn_index: usize,
    pub timestamp: Option<String>,
    pub kind: EventKind,
    pub usage: TokenUsage,
    pub tool_name: Option<String>,
    pub target_path: Option<String>,
    pub size_tokens: Option<u64>,
    pub estimated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    UserPrompt,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Boundary,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}
