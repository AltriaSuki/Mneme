//! Parse tool calls from plain-text LLM output.
//!
//! Some API proxies strip structured `tool_use` blocks. In Text/Auto mode,
//! the model outputs tool calls as `<tool_call>JSON</tool_call>` tags or
//! as JSON in markdown code blocks. This module extracts and normalises them.

use regex::Regex;
use serde_json::Value;

/// A tool call parsed from plain text.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub input: Value,
}

/// Extract tool calls from LLM text output.
///
/// Supported formats (priority order):
/// 1. `<tool_call>{"name":"shell","arguments":{"command":"ls"}}</tool_call>`
/// 2. `<tool_call>{"name":"shell","input":{"command":"ls"}}</tool_call>`
/// 3. Markdown JSON code blocks containing objects with `name`/`tool` + `arguments`/`input`
/// 4. Backtick shell commands: ````bash\nls -la\n```` or standalone `` `ls -la` ``
pub fn parse_text_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut results = Vec::new();

    // 1. <tool_call> tags (highest priority)
    let tag_re = Regex::new(r"(?si)<\s*tool_call\s*>(.*?)<\s*/\s*tool_call\s*>").unwrap();
    for caps in tag_re.captures_iter(text) {
        let inner = caps.get(1).map_or("", |m| m.as_str()).trim();
        if let Some(parsed) = try_parse_tool_json(inner) {
            results.push(parsed);
        }
    }

    if !results.is_empty() {
        return results;
    }

    // 2. Markdown JSON code blocks as fallback
    let md_re = Regex::new(r"(?s)```(?:json)?\s*\n?(.*?)```").unwrap();
    for caps in md_re.captures_iter(text) {
        let inner = caps.get(1).map_or("", |m| m.as_str()).trim();
        if let Some(parsed) = try_parse_tool_json(inner) {
            results.push(parsed);
        }
    }

    if !results.is_empty() {
        return results;
    }

    // 3. Backtick shell commands (lowest priority)
    results.extend(parse_backtick_commands(text));

    results
}

/// Strip all `<tool_call>...</tool_call>` tags and backtick shell commands from text.
pub fn strip_tool_calls(text: &str) -> String {
    // Strip <tool_call> tags
    let tag_re = Regex::new(r"(?si)<\s*tool_call\s*>.*?<\s*/\s*tool_call\s*>").unwrap();
    let result = tag_re.replace_all(text, "").to_string();

    // Strip ```bash/sh/shell/zsh blocks
    let bash_block_re = Regex::new(r"(?s)```(?:bash|sh|shell|zsh)\s*\n.*?```").unwrap();
    let result = bash_block_re.replace_all(&result, "").to_string();

    // Strip standalone single-backtick commands (whole line is `command`)
    let inline_re = Regex::new(r"(?m)^\s*`([^`\n]+)`\s*$").unwrap();
    let result = inline_re.replace_all(&result, "").to_string();

    // Clean up excess whitespace left by stripping
    let multi_newline = Regex::new(r"\n{3,}").unwrap();
    multi_newline
        .replace_all(&result, "\n\n")
        .trim()
        .to_string()
}

/// Parse backtick-wrapped shell commands from LLM text.
///
/// Detects two patterns:
/// 1. Triple-backtick blocks with bash/sh/shell/zsh language tag
/// 2. Standalone single-backtick commands on their own line
fn parse_backtick_commands(text: &str) -> Vec<ParsedToolCall> {
    let mut results = Vec::new();

    // Pattern 1: ```bash\ncommand\n```
    let bash_block_re = Regex::new(r"(?s)```(?:bash|sh|shell|zsh)\s*\n(.*?)```").unwrap();
    for caps in bash_block_re.captures_iter(text) {
        let cmd = caps.get(1).map_or("", |m| m.as_str()).trim();
        if !cmd.is_empty() {
            results.push(ParsedToolCall {
                name: "shell".to_string(),
                input: serde_json::json!({"command": cmd}),
            });
        }
    }

    if !results.is_empty() {
        return results;
    }

    // Pattern 2: standalone `command` on its own line
    let inline_re = Regex::new(r"(?m)^\s*`([^`\n]+)`\s*$").unwrap();
    for caps in inline_re.captures_iter(text) {
        let cmd = caps.get(1).map_or("", |m| m.as_str()).trim();
        if !cmd.is_empty() {
            results.push(ParsedToolCall {
                name: "shell".to_string(),
                input: serde_json::json!({"command": cmd}),
            });
        }
    }

    results
}

/// Try to parse a JSON string as a tool call object.
/// Normalises field names: `tool` → `name`, `arguments` → `input`.
fn try_parse_tool_json(json_str: &str) -> Option<ParsedToolCall> {
    let obj: Value = serde_json::from_str(json_str).ok()?;
    let map = obj.as_object()?;

    // Extract tool name: "name" or "tool"
    let name = map
        .get("name")
        .or_else(|| map.get("tool"))
        .and_then(|v| v.as_str())?
        .to_string();

    if name.is_empty() {
        return None;
    }

    // Extract input: "input" or "arguments" or "parameters"
    let input = map
        .get("input")
        .or_else(|| map.get("arguments"))
        .or_else(|| map.get("parameters"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    Some(ParsedToolCall { name, input })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call_tag_with_arguments() {
        let text =
            r#"我来看看 <tool_call>{"name":"shell","arguments":{"command":"ls -la"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].input["command"], "ls -la");
    }

    #[test]
    fn test_parse_tool_call_tag_with_input() {
        let text = r#"<tool_call>{"name":"shell","input":{"command":"git status"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].input["command"], "git status");
    }

    #[test]
    fn test_parse_tool_call_tag_with_tool_key() {
        let text = r#"<tool_call>{"tool":"browser_goto","arguments":{"url":"https://example.com"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "browser_goto");
        assert_eq!(calls[0].input["url"], "https://example.com");
    }

    #[test]
    fn test_parse_multiple_tool_calls() {
        let text = r#"先看看目录 <tool_call>{"name":"shell","arguments":{"command":"ls"}}</tool_call> 再看看状态 <tool_call>{"name":"shell","arguments":{"command":"git status"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].input["command"], "ls");
        assert_eq!(calls[1].input["command"], "git status");
    }

    #[test]
    fn test_parse_markdown_code_block_fallback() {
        let text =
            "我来执行命令：\n```json\n{\"name\":\"shell\",\"arguments\":{\"command\":\"ls\"}}\n```";
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn test_tag_takes_priority_over_markdown() {
        let text = r#"<tool_call>{"name":"shell","input":{"command":"ls"}}</tool_call>
```json
{"name":"shell","input":{"command":"pwd"}}
```"#;
        let calls = parse_text_tool_calls(text);
        // Only the tag result, markdown is skipped when tags found
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["command"], "ls");
    }

    #[test]
    fn test_parse_no_tool_calls() {
        let text = "这只是普通文本，没有工具调用";
        let calls = parse_text_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_invalid_json_ignored() {
        let text = "<tool_call>not valid json</tool_call>";
        let calls = parse_text_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_missing_name_ignored() {
        let text = r#"<tool_call>{"arguments":{"command":"ls"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_empty_name_ignored() {
        let text = r#"<tool_call>{"name":"","arguments":{"command":"ls"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_no_arguments_defaults_to_empty_object() {
        let text = r#"<tool_call>{"name":"browser_screenshot"}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "browser_screenshot");
        assert!(calls[0].input.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_strip_tool_calls() {
        let text = "前面的文字 <tool_call>{\"name\":\"shell\",\"input\":{\"command\":\"ls\"}}</tool_call> 后面的文字";
        let stripped = strip_tool_calls(text);
        assert_eq!(stripped, "前面的文字  后面的文字");
        assert!(!stripped.contains("tool_call"));
    }

    #[test]
    fn test_strip_tool_calls_case_insensitive() {
        let text = "text <Tool_Call>{\"name\":\"shell\",\"input\":{}}</Tool_Call> more";
        let stripped = strip_tool_calls(text);
        assert!(!stripped.contains("Tool_Call"));
    }

    #[test]
    fn test_strip_no_tool_calls() {
        let text = "普通文本不变";
        assert_eq!(strip_tool_calls(text), text);
    }

    #[test]
    fn test_parse_parameters_key() {
        let text = r#"<tool_call>{"name":"shell","parameters":{"command":"whoami"}}</tool_call>"#;
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["command"], "whoami");
    }

    // --- Backtick command detection ---

    #[test]
    fn test_parse_bash_code_block() {
        let text = "我来看看：\n```bash\nls -la\n```";
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].input["command"], "ls -la");
    }

    #[test]
    fn test_parse_sh_code_block() {
        let text = "```sh\ngit status\n```";
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["command"], "git status");
    }

    #[test]
    fn test_parse_standalone_backtick_command() {
        let text = "我执行一下：\n`ls -la`\n看看结果";
        let calls = parse_text_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].input["command"], "ls -la");
    }

    #[test]
    fn test_inline_backtick_not_parsed() {
        // Inline backticks within prose should NOT be parsed as commands
        let text = "你可以用 `grep` 命令来搜索文件";
        let calls = parse_text_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_tool_call_tag_takes_priority_over_backtick() {
        let text =
            "<tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"pwd\"}}</tool_call>\n`ls`";
        let calls = parse_text_tool_calls(text);
        // Only the <tool_call> tag, backtick is skipped
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["command"], "pwd");
    }

    #[test]
    fn test_strip_bash_code_block() {
        let text = "看看目录：\n```bash\nls -la\n```\n结果如上";
        let stripped = strip_tool_calls(text);
        assert!(!stripped.contains("```"));
        assert!(!stripped.contains("ls -la"));
    }

    #[test]
    fn test_strip_standalone_backtick() {
        let text = "执行：\n`pwd`\n完成";
        let stripped = strip_tool_calls(text);
        assert!(!stripped.contains("`pwd`"));
    }
}
