//! Metacognition response parsing (#24)
//!
//! Parses LLM output from metacognitive reflection into structured insights.
//! Reuses the multi-strategy JSON parsing approach from `extraction.rs`.

use serde::Deserialize;

/// A single metacognitive insight.
#[derive(Debug, Clone, Deserialize)]
pub struct MetacognitionInsight {
    /// Domain of the insight: "behavior", "emotion", "social", "expression", etc.
    pub domain: String,
    /// The insight content (natural language).
    pub content: String,
    /// Confidence in this insight (0.0-1.0).
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// Whether this insight should be kept private (not injected into prompts).
    /// Auto-detected for emotion/body_feeling domains; LLM can also flag explicitly.
    #[serde(default)]
    pub is_private: bool,
}

fn default_confidence() -> f32 {
    0.6
}

/// Wrapper for the LLM response.
#[derive(Debug, Deserialize)]
struct MetacognitionResponse {
    insights: Vec<MetacognitionInsight>,
}

/// Parse the LLM's metacognition response into structured insights.
///
/// Strategies (tried in order, same as extraction.rs):
/// 1. Direct JSON parse
/// 2. Extract JSON from markdown code block
/// 3. Find outermost `{...}` and parse
/// 4. Find outermost `[...]` as bare array
/// 5. Fix common JSON issues and retry
/// 6. Graceful fallback: empty vec
pub fn parse_metacognition_response(text: &str) -> Vec<MetacognitionInsight> {
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return Vec::new();
    }

    // Strategy 1: Direct parse
    if let Ok(resp) = serde_json::from_str::<MetacognitionResponse>(trimmed) {
        return resp.insights;
    }

    // Strategy 2: Extract from markdown code block
    let code_block_re = regex::Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?\s*```").unwrap();
    if let Some(caps) = code_block_re.captures(trimmed) {
        let inner = caps.get(1).map_or("", |m| m.as_str()).trim();
        if let Ok(resp) = serde_json::from_str::<MetacognitionResponse>(inner) {
            return resp.insights;
        }
    }

    // Strategy 3: Extract outermost {...}
    if let Some(json_str) = extract_balanced_braces(trimmed) {
        if let Ok(resp) = serde_json::from_str::<MetacognitionResponse>(&json_str) {
            return resp.insights;
        }
        let repaired = repair_json(&json_str);
        if let Ok(resp) = serde_json::from_str::<MetacognitionResponse>(&repaired) {
            return resp.insights;
        }
    }

    // Strategy 4: Bare array [...]
    if let Some(arr_start) = trimmed.find('[') {
        if let Some(arr_end) = trimmed.rfind(']') {
            let arr_str = &trimmed[arr_start..=arr_end];
            if let Ok(insights) = serde_json::from_str::<Vec<MetacognitionInsight>>(arr_str) {
                return insights;
            }
            let repaired = repair_json(arr_str);
            if let Ok(insights) = serde_json::from_str::<Vec<MetacognitionInsight>>(&repaired) {
                return insights;
            }
        }
    }

    // Strategy 5: Full text repair and retry
    let repaired = repair_json(trimmed);
    if let Ok(resp) = serde_json::from_str::<MetacognitionResponse>(&repaired) {
        return resp.insights;
    }

    tracing::debug!(
        "Could not parse metacognition response: {}",
        &trimmed[..trimmed.len().min(200)]
    );
    Vec::new()
}

/// Format insights into a human-readable summary for episode storage.
pub fn format_metacognition_summary(insights: &[MetacognitionInsight]) -> String {
    if insights.is_empty() {
        return "元认知反思未产生新洞察。".to_string();
    }
    let mut lines = vec!["元认知反思结果：".to_string()];
    for (i, insight) in insights.iter().enumerate() {
        lines.push(format!(
            "{}. [{}] {} (置信度: {:.0}%)",
            i + 1,
            insight.domain,
            insight.content,
            insight.confidence * 100.0,
        ));
    }
    lines.join("\n")
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

    // Remove trailing commas before } or ]
    let trailing_comma = regex::Regex::new(r",\s*([}\]])").unwrap();
    result = trailing_comma.replace_all(&result, "$1").to_string();

    // Replace single quotes with double quotes if no double quotes present
    if !result.contains('"') {
        result = result.replace('\'', "\"");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let json = r#"{"insights": [{"domain": "behavior", "content": "倾向于在压力下回避社交", "confidence": 0.8}]}"#;
        let insights = parse_metacognition_response(json);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].domain, "behavior");
        assert!((insights[0].confidence - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_parse_code_block() {
        let text = "以下是反思结果：\n```json\n{\"insights\": [{\"domain\": \"emotion\", \"content\": \"情绪波动较大\", \"confidence\": 0.7}]}\n```";
        let insights = parse_metacognition_response(text);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].domain, "emotion");
    }

    #[test]
    fn test_parse_empty_insights() {
        let json = r#"{"insights": []}"#;
        let insights = parse_metacognition_response(json);
        assert!(insights.is_empty());
    }

    #[test]
    fn test_parse_garbage_returns_empty() {
        let insights = parse_metacognition_response("I have no structured output to give");
        assert!(insights.is_empty());
    }

    #[test]
    fn test_parse_missing_confidence_default() {
        let json = r#"{"insights": [{"domain": "social", "content": "需要更多主动社交"}]}"#;
        let insights = parse_metacognition_response(json);
        assert_eq!(insights.len(), 1);
        assert!((insights[0].confidence - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_parse_private_field() {
        let json = r#"{"insights": [{"domain": "emotion", "content": "内心深处的不安", "confidence": 0.7, "is_private": true}]}"#;
        let insights = parse_metacognition_response(json);
        assert_eq!(insights.len(), 1);
        assert!(insights[0].is_private);
    }

    #[test]
    fn test_parse_private_field_default_false() {
        let json =
            r#"{"insights": [{"domain": "behavior", "content": "test", "confidence": 0.5}]}"#;
        let insights = parse_metacognition_response(json);
        assert_eq!(insights.len(), 1);
        assert!(!insights[0].is_private);
    }

    #[test]
    fn test_format_summary() {
        let insights = vec![
            MetacognitionInsight {
                domain: "behavior".to_string(),
                content: "回避倾向".to_string(),
                confidence: 0.8,
                is_private: false,
            },
            MetacognitionInsight {
                domain: "emotion".to_string(),
                content: "情绪稳定性提高".to_string(),
                confidence: 0.6,
                is_private: false,
            },
        ];
        let summary = format_metacognition_summary(&insights);
        assert!(summary.contains("元认知反思结果"));
        assert!(summary.contains("[behavior]"));
        assert!(summary.contains("[emotion]"));
        assert!(summary.contains("80%"));
        assert!(summary.contains("60%"));
    }
}
