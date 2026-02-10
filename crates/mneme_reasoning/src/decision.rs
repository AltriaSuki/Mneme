// ============================================================================
// Decision levels
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionLevel {
    /// Rule match: direct response without LLM (e.g. CLI commands)
    RuleMatch(String),
    /// Quick response: simple greeting/confirmation (low max_tokens)
    QuickResponse,
    /// Full reasoning: needs LLM + tools + memory retrieval
    FullReasoning,
}

// ============================================================================
// DecisionRule trait
// ============================================================================

pub trait DecisionRule: Send + Sync {
    /// Evaluate input and return a decision level, or None to pass to next rule.
    fn evaluate(&self, input: &str) -> Option<DecisionLevel>;

    /// Name for logging.
    fn name(&self) -> &str;
}

// ============================================================================
// DecisionRouter
// ============================================================================

pub struct DecisionRouter {
    rules: Vec<Box<dyn DecisionRule>>,
}

impl Default for DecisionRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionRouter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Create a router with the default built-in rules.
    pub fn with_defaults() -> Self {
        let mut router = Self::new();
        router.add_rule(Box::new(EmptyInputRule));
        router.add_rule(Box::new(GreetingRule));
        router
    }

    pub fn add_rule(&mut self, rule: Box<dyn DecisionRule>) {
        self.rules.push(rule);
    }

    /// Route input through rules in order. First match wins.
    /// Falls back to FullReasoning if no rule matches.
    pub fn route(&self, input: &str) -> DecisionLevel {
        for rule in &self.rules {
            if let Some(level) = rule.evaluate(input) {
                tracing::debug!("DecisionRouter: rule '{}' matched → {:?}", rule.name(), level);
                return level;
            }
        }
        DecisionLevel::FullReasoning
    }
}

// ============================================================================
// Built-in rules
// ============================================================================

/// Filters empty or whitespace-only input.
pub struct EmptyInputRule;

impl DecisionRule for EmptyInputRule {
    fn evaluate(&self, input: &str) -> Option<DecisionLevel> {
        if input.trim().is_empty() {
            Some(DecisionLevel::RuleMatch(String::new()))
        } else {
            None
        }
    }

    fn name(&self) -> &str { "empty_input" }
}

/// Detects simple greetings that can use a lower token budget.
pub struct GreetingRule;

impl DecisionRule for GreetingRule {
    fn evaluate(&self, input: &str) -> Option<DecisionLevel> {
        let trimmed = input.trim();
        // Only match short inputs (greetings are brief)
        if trimmed.chars().count() > 20 {
            return None;
        }
        let lower = trimmed.to_lowercase();
        const GREETINGS: &[&str] = &[
            "你好", "hi", "hello", "hey", "嗨", "早上好", "晚上好",
            "下午好", "早安", "晚安", "在吗", "在不在",
        ];
        if GREETINGS.iter().any(|g| lower == *g) {
            Some(DecisionLevel::QuickResponse)
        } else {
            None
        }
    }

    fn name(&self) -> &str { "greeting" }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_rule() {
        let rule = EmptyInputRule;
        assert_eq!(rule.evaluate(""), Some(DecisionLevel::RuleMatch(String::new())));
        assert_eq!(rule.evaluate("   "), Some(DecisionLevel::RuleMatch(String::new())));
        assert_eq!(rule.evaluate("hello"), None);
    }

    #[test]
    fn test_greeting_rule_chinese() {
        let rule = GreetingRule;
        assert_eq!(rule.evaluate("你好"), Some(DecisionLevel::QuickResponse));
        assert_eq!(rule.evaluate("早上好"), Some(DecisionLevel::QuickResponse));
        assert_eq!(rule.evaluate("在吗"), Some(DecisionLevel::QuickResponse));
    }

    #[test]
    fn test_greeting_rule_english() {
        let rule = GreetingRule;
        assert_eq!(rule.evaluate("hi"), Some(DecisionLevel::QuickResponse));
        assert_eq!(rule.evaluate("Hello"), Some(DecisionLevel::QuickResponse));
        assert_eq!(rule.evaluate("HEY"), Some(DecisionLevel::QuickResponse));
    }

    #[test]
    fn test_greeting_rule_rejects_long_input() {
        let rule = GreetingRule;
        assert_eq!(rule.evaluate("你好，今天天气怎么样？我想出去走走"), None);
    }

    #[test]
    fn test_greeting_rule_rejects_non_greeting() {
        let rule = GreetingRule;
        assert_eq!(rule.evaluate("帮我写代码"), None);
        assert_eq!(rule.evaluate("what is rust"), None);
    }

    #[test]
    fn test_router_empty_rules_defaults_to_full() {
        let router = DecisionRouter::new();
        assert_eq!(router.route("anything"), DecisionLevel::FullReasoning);
    }

    #[test]
    fn test_router_first_match_wins() {
        let router = DecisionRouter::with_defaults();
        // Empty input matches EmptyInputRule first
        assert_eq!(router.route(""), DecisionLevel::RuleMatch(String::new()));
        // Greeting matches GreetingRule
        assert_eq!(router.route("你好"), DecisionLevel::QuickResponse);
        // Complex input falls through to FullReasoning
        assert_eq!(router.route("请帮我分析这段代码"), DecisionLevel::FullReasoning);
    }

    #[test]
    fn test_router_with_custom_rule() {
        struct AlwaysQuick;
        impl DecisionRule for AlwaysQuick {
            fn evaluate(&self, _input: &str) -> Option<DecisionLevel> {
                Some(DecisionLevel::QuickResponse)
            }
            fn name(&self) -> &str { "always_quick" }
        }

        let mut router = DecisionRouter::new();
        router.add_rule(Box::new(AlwaysQuick));
        assert_eq!(router.route("anything at all"), DecisionLevel::QuickResponse);
    }
}
