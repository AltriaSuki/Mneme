//! Fact & goal extraction: post-conversation LLM call to extract structured data.
//!
//! Runs asynchronously after `think()` completes. Uses a minimal prompt to extract
//! factual triples and conversational goals from the latest exchange.

use crate::api_types::{ContentBlock, Message, Role};
use crate::llm::{CompletionParams, LlmClient};
use anyhow::{Context, Result};
use serde::Deserialize;

/// A single extracted fact triple.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtractedFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// How confident the LLM is about this fact (0.0-1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

/// An extracted goal from conversation.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtractedGoal {
    /// Goal description
    pub description: String,
    /// "achievement", "social", "exploration", "maintenance"
    #[serde(default = "default_goal_type")]
    pub goal_type: String,
    /// Priority 0.0-1.0
    #[serde(default = "default_priority")]
    pub priority: f32,
}

fn default_confidence() -> f32 {
    0.7
}
fn default_goal_type() -> String {
    "achievement".to_string()
}
fn default_priority() -> f32 {
    0.5
}

/// Extraction response from the LLM.
#[derive(Debug, Deserialize)]
struct ExtractionResponse {
    #[serde(default)]
    facts: Vec<ExtractedFact>,
    #[serde(default)]
    goals: Vec<ExtractedGoal>,
}

const EXTRACTION_SYSTEM_PROMPT: &str = r#"你是一个信息提取模块。给你一段对话，请提取事实性信息和对话中提到的目标/计划。

事实规则：
1. 只提取明确陈述的事实，不要推测
2. subject 通常是"用户"或具体人名
3. predicate 用简短动词短语，如"喜欢""住在""在做""讨厌""是""有"
4. object 是具体内容
5. confidence 表示确定程度：直接陈述=0.9，语气不确定=0.5，隐含暗示=0.3
6. 不要提取聊天中的客套、问候、情绪表达

目标规则：
1. 只提取用户明确表达的意图、计划、想做的事
2. goal_type: "achievement"(一次性目标), "social"(社交), "exploration"(探索学习), "maintenance"(持续维护)
3. priority: 0.3=随口一提, 0.5=一般, 0.7=明确想做, 0.9=很重要
4. 如果没有目标，返回空数组

用 JSON 格式返回：
{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}], "goals": [{"description": "学习Rust", "goal_type": "exploration", "priority": 0.7}]}"#;

/// Extract facts from a recent conversation exchange (backward-compatible wrapper).
pub async fn extract_facts(
    client: &dyn LlmClient,
    user_text: &str,
    assistant_reply: &str,
) -> Vec<ExtractedFact> {
    let (facts, _) = extract_all(client, user_text, assistant_reply).await;
    facts
}

/// Extract both facts and goals from a conversation exchange in a single LLM call.
pub async fn extract_all(
    client: &dyn LlmClient,
    user_text: &str,
    assistant_reply: &str,
) -> (Vec<ExtractedFact>, Vec<ExtractedGoal>) {
    match extract_inner(client, user_text, assistant_reply).await {
        Ok((facts, goals)) => (facts, goals),
        Err(e) => {
            tracing::warn!("Extraction failed (non-fatal): {}", e);
            (Vec::new(), Vec::new())
        }
    }
}

async fn extract_inner(
    client: &dyn LlmClient,
    user_text: &str,
    assistant_reply: &str,
) -> Result<(Vec<ExtractedFact>, Vec<ExtractedGoal>)> {
    // Skip extraction for very short exchanges (greetings, etc.)
    if user_text.len() < 5 {
        return Ok((Vec::new(), Vec::new()));
    }

    let conversation = format!("用户: {}\n回复: {}", user_text, assistant_reply);

    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: conversation }],
    }];

    let params = CompletionParams {
        max_tokens: 512,  // Extraction is brief
        temperature: 0.1, // Low temperature for structured output
    };

    let response = client
        .complete(EXTRACTION_SYSTEM_PROMPT, messages, vec![], params)
        .await
        .context("Extraction LLM call failed")?;

    // Parse the response text as JSON
    let response_text = response
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");

    // Try to parse JSON, being lenient about formatting
    let (facts_raw, goals_raw) = parse_extraction_response(&response_text)?;

    let valid_facts: Vec<ExtractedFact> = facts_raw
        .into_iter()
        .filter(|f| {
            !f.subject.is_empty()
                && !f.predicate.is_empty()
                && !f.object.is_empty()
                && f.confidence > 0.0
                && f.confidence <= 1.0
        })
        .collect();

    let valid_goals: Vec<ExtractedGoal> = goals_raw
        .into_iter()
        .filter(|g| !g.description.is_empty() && g.priority > 0.0)
        .collect();

    tracing::debug!(
        "Extracted {} facts, {} goals from conversation",
        valid_facts.len(),
        valid_goals.len()
    );
    Ok((valid_facts, valid_goals))
}

/// Parse the LLM's response, handling common formatting quirks.
///
/// Strategies (tried in order):
/// 1. Direct JSON parse
/// 2. Extract JSON from markdown code block (```json ... ```)
/// 3. Find outermost `{...}` and parse
/// 4. Find outermost `[...]` as bare array
/// 5. Fix common JSON issues (trailing commas, single quotes) and retry
/// 6. Graceful fallback: empty vec
pub fn parse_extraction_response(text: &str) -> Result<(Vec<ExtractedFact>, Vec<ExtractedGoal>)> {
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Helper: try parsing as ExtractionResponse (has both facts + goals with defaults)
    let try_parse = |s: &str| -> Option<(Vec<ExtractedFact>, Vec<ExtractedGoal>)> {
        serde_json::from_str::<ExtractionResponse>(s)
            .ok()
            .filter(|r| !r.facts.is_empty() || !r.goals.is_empty())
            .map(|r| (r.facts, r.goals))
    };

    // Strategy 1: Direct parse
    if let Some(r) = try_parse(trimmed) { return Ok(r); }

    // Strategy 2: Extract from markdown code block
    let code_block_re = regex::Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?\s*```").unwrap();
    if let Some(caps) = code_block_re.captures(trimmed) {
        let inner = caps.get(1).map_or("", |m| m.as_str()).trim();
        if let Some(r) = try_parse(inner) { return Ok(r); }
    }

    // Strategy 3: Extract outermost {...}
    if let Some(json_str) = extract_balanced_braces(trimmed) {
        if let Some(r) = try_parse(&json_str) { return Ok(r); }
        let repaired = repair_json(&json_str);
        if let Some(r) = try_parse(&repaired) { return Ok(r); }
    }

    // Strategy 4: Bare array [...] — facts only (no goals)
    if let Some(arr_start) = trimmed.find('[') {
        if let Some(arr_end) = trimmed.rfind(']') {
            let arr_str = &trimmed[arr_start..=arr_end];
            if let Ok(facts) = serde_json::from_str::<Vec<ExtractedFact>>(arr_str) {
                return Ok((facts, Vec::new()));
            }
            let repaired = repair_json(arr_str);
            if let Ok(facts) = serde_json::from_str::<Vec<ExtractedFact>>(&repaired) {
                return Ok((facts, Vec::new()));
            }
        }
    }

    // Strategy 5: Full text repair and retry
    let repaired = repair_json(trimmed);
    if let Some(r) = try_parse(&repaired) { return Ok(r); }

    tracing::debug!(
        "Could not parse extraction response: {}",
        &trimmed[..trimmed.len().min(200)]
    );
    Ok((Vec::new(), Vec::new()))
}

/// Extract the outermost balanced `{...}` substring.
fn extract_balanced_braces(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text[start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Attempt to repair common JSON formatting issues from LLM output.
fn repair_json(text: &str) -> String {
    let mut result = text.to_string();

    // 1. Remove trailing commas before } or ]
    let trailing_comma = regex::Regex::new(r",\s*([}\]])").unwrap();
    result = trailing_comma.replace_all(&result, "$1").to_string();

    // 2. Replace single quotes with double quotes (outside already-double-quoted strings)
    //    Simple heuristic: if text has no double quotes at all, replace singles
    if !result.contains('"') {
        result = result.replace('\'', "\"");
    }

    // 3. Handle unquoted keys: { key: "value" } → { "key": "value" }
    let unquoted_key = regex::Regex::new(r"(?m)\{\s*(\w+)\s*:|\,\s*(\w+)\s*:").unwrap();
    result = unquoted_key
        .replace_all(&result, |caps: &regex::Captures| {
            let key = caps.get(1).or(caps.get(2)).map_or("", |m| m.as_str());
            if caps.get(0).unwrap().as_str().starts_with('{') {
                format!("{{\"{}\":", key)
            } else {
                format!(",\"{}\":", key)
            }
        })
        .to_string();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let json = r#"{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}]}"#;
        let (facts, _) = parse_extraction_response(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "用户");
        assert_eq!(facts[0].predicate, "喜欢");
        assert_eq!(facts[0].object, "猫");
        assert!((facts[0].confidence - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_parse_code_block_wrapped() {
        let text = "```json\n{\"facts\": [{\"subject\": \"用户\", \"predicate\": \"住在\", \"object\": \"上海\", \"confidence\": 0.8}]}\n```";
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "上海");
    }

    #[test]
    fn test_parse_empty_facts() {
        let json = r#"{"facts": []}"#;
        let (facts, _) = parse_extraction_response(json).unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_garbage_returns_empty() {
        let (facts, _) = parse_extraction_response("I don't know how to parse this").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_missing_confidence_uses_default() {
        let json = r#"{"facts": [{"subject": "用户", "predicate": "是", "object": "程序员"}]}"#;
        let (facts, _) = parse_extraction_response(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert!((facts[0].confidence - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_filter_invalid_facts() {
        // Facts with empty fields should be filtered out in extract_facts_inner validation
        let json = r#"{"facts": [
            {"subject": "", "predicate": "喜欢", "object": "猫", "confidence": 0.9},
            {"subject": "用户", "predicate": "喜欢", "object": "狗", "confidence": 0.8}
        ]}"#;
        let (all, _) = parse_extraction_response(json).unwrap();
        assert_eq!(all.len(), 2); // Parser returns all

        // But the validation filter would keep only the valid one
        let valid: Vec<_> = all
            .into_iter()
            .filter(|f| !f.subject.is_empty() && !f.predicate.is_empty() && !f.object.is_empty())
            .collect();
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].object, "狗");
    }

    // --- New #8 robustness tests ---

    #[test]
    fn test_parse_trailing_comma() {
        let json = r#"{"facts": [{"subject": "用户", "predicate": "住在", "object": "北京", "confidence": 0.8},]}"#;
        let (facts, _) = parse_extraction_response(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "北京");
    }

    #[test]
    fn test_parse_with_preamble_text() {
        let text = "好的，我来提取事实：\n\n{\"facts\": [{\"subject\": \"用户\", \"predicate\": \"是\", \"object\": \"程序员\", \"confidence\": 0.9}]}";
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "程序员");
    }

    #[test]
    fn test_parse_markdown_code_block_with_language() {
        let text = "提取结果如下：\n```json\n{\"facts\": [{\"subject\": \"用户\", \"predicate\": \"喜欢\", \"object\": \"Rust\", \"confidence\": 0.95}]}\n```\n以上就是提取的事实。";
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "Rust");
    }

    #[test]
    fn test_parse_bare_code_block() {
        let text = "```\n{\"facts\": [{\"subject\": \"小明\", \"predicate\": \"养了\", \"object\": \"一只猫\", \"confidence\": 0.85}]}\n```";
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "小明");
    }

    #[test]
    fn test_parse_nested_braces_in_object() {
        // JSON with nested braces in the data (should find correct outer braces)
        let text = r#"{"facts": [{"subject": "用户", "predicate": "写了", "object": "fn main() { println!(\"hello\") }", "confidence": 0.7}]}"#;
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert!(facts[0].object.contains("fn main"));
    }

    #[test]
    fn test_parse_empty_string() {
        let (facts, _) = parse_extraction_response("").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let (facts, _) = parse_extraction_response("   \n\n  ").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_bare_array() {
        let text =
            r#"[{"subject": "用户", "predicate": "住在", "object": "上海", "confidence": 0.9}]"#;
        let (facts, _) = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_repair_trailing_comma_in_array() {
        // Trailing comma inside the array
        let json = r#"{"facts": [
            {"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9},
            {"subject": "用户", "predicate": "养了", "object": "一只狗", "confidence": 0.8},
        ]}"#;
        let (facts, _) = parse_extraction_response(json).unwrap();
        assert_eq!(facts.len(), 2);
    }
}
