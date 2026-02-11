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
    fn test_router_first_match_wins() {
        let router = DecisionRouter::with_defaults();
        // Empty input matches EmptyInputRule first
        assert_eq!(router.route(""), DecisionLevel::RuleMatch(String::new()));
        // Non-empty input falls through to FullReasoning (no greeting shortcut)
        assert_eq!(router.route("你好"), DecisionLevel::FullReasoning);
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
