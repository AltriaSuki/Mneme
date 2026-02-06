//! Fact extraction: post-conversation LLM call to extract (subject, predicate, object) triples.
//!
//! Runs asynchronously after `think()` completes. Uses a minimal prompt to extract
//! factual information from the latest exchange and stores it in semantic memory.

use crate::llm::{LlmClient, CompletionParams};
use crate::api_types::{Message, Role, ContentBlock};
use anyhow::{Result, Context};
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

fn default_confidence() -> f32 {
    0.7
}

/// Extraction response from the LLM.
#[derive(Debug, Deserialize)]
struct ExtractionResponse {
    facts: Vec<ExtractedFact>,
}

const EXTRACTION_SYSTEM_PROMPT: &str = r#"你是一个信息提取模块。给你一段对话，请提取出其中的事实性信息。

规则：
1. 只提取明确陈述的事实，不要推测
2. subject 通常是"用户"或具体人名
3. predicate 用简短动词短语，如"喜欢""住在""在做""讨厌""是""有"
4. object 是具体内容
5. confidence 表示确定程度：直接陈述=0.9，语气不确定=0.5，隐含暗示=0.3
6. 如果没有可提取的事实，返回空数组
7. 不要提取聊天中的客套、问候、情绪表达

用 JSON 格式返回：
{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}]}"#;

/// Extract facts from a recent conversation exchange.
///
/// Takes the user's message and the assistant's reply, sends a lightweight
/// LLM call to extract factual triples.
///
/// Returns an empty vec if no facts are found or if extraction fails gracefully.
pub async fn extract_facts(
    client: &dyn LlmClient,
    user_text: &str,
    assistant_reply: &str,
) -> Vec<ExtractedFact> {
    match extract_facts_inner(client, user_text, assistant_reply).await {
        Ok(facts) => facts,
        Err(e) => {
            tracing::warn!("Fact extraction failed (non-fatal): {}", e);
            Vec::new()
        }
    }
}

async fn extract_facts_inner(
    client: &dyn LlmClient,
    user_text: &str,
    assistant_reply: &str,
) -> Result<Vec<ExtractedFact>> {
    // Skip extraction for very short exchanges (greetings, etc.)
    if user_text.len() < 5 {
        return Ok(Vec::new());
    }

    let conversation = format!("用户: {}\n回复: {}", user_text, assistant_reply);

    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: conversation,
        }],
    }];

    let params = CompletionParams {
        max_tokens: 512, // Extraction is brief
        temperature: 0.1, // Low temperature for structured output
    };

    let response = client
        .complete(EXTRACTION_SYSTEM_PROMPT, messages, vec![], params)
        .await
        .context("Extraction LLM call failed")?;

    // Parse the response text as JSON
    let response_text = response.content.iter()
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
    let parsed = parse_extraction_response(&response_text)?;

    // Validate and filter facts
    let valid_facts: Vec<ExtractedFact> = parsed
        .into_iter()
        .filter(|f| {
            !f.subject.is_empty()
                && !f.predicate.is_empty()
                && !f.object.is_empty()
                && f.confidence > 0.0
                && f.confidence <= 1.0
        })
        .collect();

    tracing::debug!("Extracted {} valid facts from conversation", valid_facts.len());
    Ok(valid_facts)
}

/// Parse the LLM's response, handling common formatting quirks.
fn parse_extraction_response(text: &str) -> Result<Vec<ExtractedFact>> {
    let trimmed = text.trim();

    // Try direct parse first
    if let Ok(resp) = serde_json::from_str::<ExtractionResponse>(trimmed) {
        return Ok(resp.facts);
    }

    // Try extracting JSON from markdown code block
    if let Some(json_start) = trimmed.find('{') {
        if let Some(json_end) = trimmed.rfind('}') {
            let json_str = &trimmed[json_start..=json_end];
            if let Ok(resp) = serde_json::from_str::<ExtractionResponse>(json_str) {
                return Ok(resp.facts);
            }
        }
    }

    // Try parsing as a bare array
    if let Some(arr_start) = trimmed.find('[') {
        if let Some(arr_end) = trimmed.rfind(']') {
            let arr_str = &trimmed[arr_start..=arr_end];
            if let Ok(facts) = serde_json::from_str::<Vec<ExtractedFact>>(arr_str) {
                return Ok(facts);
            }
        }
    }

    tracing::debug!("Could not parse extraction response: {}", trimmed);
    Ok(Vec::new()) // Graceful fallback: no facts rather than error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let json = r#"{"facts": [{"subject": "用户", "predicate": "喜欢", "object": "猫", "confidence": 0.9}]}"#;
        let facts = parse_extraction_response(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "用户");
        assert_eq!(facts[0].predicate, "喜欢");
        assert_eq!(facts[0].object, "猫");
        assert!((facts[0].confidence - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_parse_code_block_wrapped() {
        let text = "```json\n{\"facts\": [{\"subject\": \"用户\", \"predicate\": \"住在\", \"object\": \"上海\", \"confidence\": 0.8}]}\n```";
        let facts = parse_extraction_response(text).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "上海");
    }

    #[test]
    fn test_parse_empty_facts() {
        let json = r#"{"facts": []}"#;
        let facts = parse_extraction_response(json).unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_garbage_returns_empty() {
        let facts = parse_extraction_response("I don't know how to parse this").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_missing_confidence_uses_default() {
        let json = r#"{"facts": [{"subject": "用户", "predicate": "是", "object": "程序员"}]}"#;
        let facts = parse_extraction_response(json).unwrap();
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
        let all = parse_extraction_response(json).unwrap();
        assert_eq!(all.len(), 2); // Parser returns all

        // But the validation filter would keep only the valid one
        let valid: Vec<_> = all.into_iter().filter(|f| {
            !f.subject.is_empty() && !f.predicate.is_empty() && !f.object.is_empty()
        }).collect();
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].object, "狗");
    }
}
