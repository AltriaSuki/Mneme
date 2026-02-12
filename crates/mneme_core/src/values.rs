//! Value System - Extensible moral reasoning and value judgment
//!
//! This module provides the foundation for value-based decision making.
//! The design separates:
//! - ValueNetwork: stores the actual value weights (state)
//! - ValueJudge: evaluates situations against values (logic)
//!
//! The ValueJudge trait allows swapping implementations:
//! - RuleBasedJudge: hardcoded rules (current baseline)
//! - Future: NeuralJudge with learned embeddings
//!
//! Key concepts from temp.md:
//! - Values form a hierarchy (core → derived)
//! - Value conflicts are natural and must be handled
//! - The system learns from feedback which values matter more

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export from state.rs
pub use crate::state::{ValueEntry, ValueNetwork};

/// A situation to be evaluated by the value system
#[derive(Debug, Clone)]
pub struct Situation {
    /// Brief description of the situation
    pub description: String,

    /// Potential action being considered
    pub proposed_action: String,

    /// Extracted keywords/features for rule matching
    pub features: Vec<String>,

    /// Context: who is involved
    pub actors: Vec<String>,

    /// Emotional context
    pub emotional_valence: f32,
}

/// Result of evaluating a situation against values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentResult {
    /// Values that would be supported by the action
    pub supported_values: Vec<ValueImpact>,

    /// Values that would be violated by the action
    pub violated_values: Vec<ValueImpact>,

    /// Overall moral valence (-1.0 to 1.0)
    /// Positive = action aligns with values
    /// Negative = action conflicts with values
    pub moral_valence: f32,

    /// Conflict detected between values
    pub has_conflict: bool,

    /// Explanation (for debugging/transparency)
    pub explanation: String,
}

/// Impact on a specific value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueImpact {
    pub value_name: String,
    pub impact_strength: f32, // 0.0 to 1.0
    pub reason: String,
}

/// Trait for value judgment - can be implemented by rules or neural networks
pub trait ValueJudge: Send + Sync {
    /// Evaluate a situation against values
    fn evaluate(&self, situation: &Situation, values: &ValueNetwork) -> JudgmentResult;

    /// Detect conflicts between values in this context
    fn detect_conflicts(&self, situation: &Situation, values: &ValueNetwork) -> Vec<ValueConflict>;

    /// Suggest how to resolve a conflict (returns which value to prioritize)
    fn resolve_conflict(&self, conflict: &ValueConflict, values: &ValueNetwork) -> String;
}

/// A conflict between two values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueConflict {
    pub value_a: String,
    pub value_b: String,
    pub context: String,
    pub severity: f32, // 0.0 to 1.0
}

// =============================================================================
// Rule-Based Implementation (Baseline)
// =============================================================================

/// Rule-based value judge - uses hardcoded keyword matching
///
/// This is the baseline implementation. Future versions can use:
/// - Embedding similarity for more nuanced matching
/// - Fine-tuned classifier for value detection
/// - Learned value hierarchies
pub struct RuleBasedJudge {
    /// Keyword patterns for each value
    value_patterns: HashMap<String, ValuePatterns>,

    /// Known conflict pairs
    conflict_pairs: Vec<(String, String, String)>, // (value_a, value_b, context_keyword)
}

/// Pattern matching rules for a value
struct ValuePatterns {
    /// Keywords that indicate support for this value
    support_keywords: Vec<String>,

    /// Keywords that indicate violation of this value
    violation_keywords: Vec<String>,
}

impl Default for RuleBasedJudge {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleBasedJudge {
    pub fn new() -> Self {
        let mut value_patterns = HashMap::new();

        // Honesty patterns
        value_patterns.insert(
            "honesty".to_string(),
            ValuePatterns {
                support_keywords: vec!["真实", "坦诚", "直说", "实话", "透明", "不隐瞒"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["撒谎", "欺骗", "隐瞒", "假装", "编造", "说谎"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Kindness patterns
        value_patterns.insert(
            "kindness".to_string(),
            ValuePatterns {
                support_keywords: vec!["帮助", "关心", "体贴", "善良", "温暖", "照顾"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["伤害", "冷漠", "忽视", "残忍", "嘲笑", "讽刺"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Authenticity patterns
        value_patterns.insert(
            "authenticity".to_string(),
            ValuePatterns {
                support_keywords: vec!["真正", "自我", "真心", "本色", "坦然"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["伪装", "迎合", "违心", "做作", "压抑"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Autonomy patterns
        value_patterns.insert(
            "autonomy".to_string(),
            ValuePatterns {
                support_keywords: vec!["选择", "自主", "决定", "独立", "自由"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["强迫", "控制", "服从", "依赖", "被动"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Growth patterns
        value_patterns.insert(
            "growth".to_string(),
            ValuePatterns {
                support_keywords: vec!["学习", "进步", "尝试", "挑战", "改变"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["停滞", "逃避", "放弃", "固守", "退缩"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Connection patterns
        value_patterns.insert(
            "connection".to_string(),
            ValuePatterns {
                support_keywords: vec!["分享", "倾听", "理解", "陪伴", "交流"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["疏远", "封闭", "隔阂", "孤立", "拒绝"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Curiosity patterns
        value_patterns.insert(
            "curiosity".to_string(),
            ValuePatterns {
                support_keywords: vec!["探索", "好奇", "发现", "为什么", "研究"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                violation_keywords: vec!["无聊", "敷衍", "忽略", "不在乎"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            },
        );

        // Common conflict pairs
        let conflict_pairs = vec![
            (
                "honesty".to_string(),
                "kindness".to_string(),
                "残忍的真相".to_string(),
            ),
            (
                "authenticity".to_string(),
                "connection".to_string(),
                "不被接受".to_string(),
            ),
            (
                "autonomy".to_string(),
                "connection".to_string(),
                "依赖".to_string(),
            ),
            (
                "growth".to_string(),
                "安全".to_string(),
                "舒适区".to_string(),
            ),
        ];

        Self {
            value_patterns,
            conflict_pairs,
        }
    }

    /// Match text against patterns
    fn match_patterns(&self, text: &str, patterns: &ValuePatterns) -> (f32, f32) {
        let mut support_score = 0.0;
        let mut violation_score = 0.0;

        for keyword in &patterns.support_keywords {
            if text.contains(keyword) {
                support_score += 1.0;
            }
        }

        for keyword in &patterns.violation_keywords {
            if text.contains(keyword) {
                violation_score += 1.0;
            }
        }

        // Normalize
        let support_max = patterns.support_keywords.len().max(1) as f32;
        let violation_max = patterns.violation_keywords.len().max(1) as f32;

        (support_score / support_max, violation_score / violation_max)
    }
}

impl ValueJudge for RuleBasedJudge {
    fn evaluate(&self, situation: &Situation, values: &ValueNetwork) -> JudgmentResult {
        let combined_text = format!(
            "{} {} {}",
            situation.description,
            situation.proposed_action,
            situation.features.join(" ")
        );

        let mut supported = Vec::new();
        let mut violated = Vec::new();
        let mut total_positive = 0.0;
        let mut total_negative = 0.0;

        for (value_name, entry) in &values.values {
            if let Some(patterns) = self.value_patterns.get(value_name) {
                let (support, violation) = self.match_patterns(&combined_text, patterns);

                if support > 0.0 {
                    let impact = support * entry.weight;
                    supported.push(ValueImpact {
                        value_name: value_name.clone(),
                        impact_strength: impact,
                        reason: format!("行动支持{}价值", value_name),
                    });
                    total_positive += impact;
                }

                if violation > 0.0 {
                    let impact = violation * entry.weight;
                    violated.push(ValueImpact {
                        value_name: value_name.clone(),
                        impact_strength: impact,
                        reason: format!("行动可能违背{}价值", value_name),
                    });
                    total_negative += impact;
                }
            }
        }

        let conflicts = self.detect_conflicts(situation, values);
        let has_conflict = !conflicts.is_empty();

        // Compute moral valence
        let moral_valence = if total_positive + total_negative > 0.0 {
            (total_positive - total_negative) / (total_positive + total_negative + 0.001)
        } else {
            0.0 // Neutral if no value impact detected
        };

        // Generate explanation
        let explanation = if has_conflict {
            format!(
                "检测到价值冲突：{}。支持的价值：{}，可能违背的价值：{}",
                conflicts
                    .iter()
                    .map(|c| format!("{} vs {}", c.value_a, c.value_b))
                    .collect::<Vec<_>>()
                    .join("; "),
                supported
                    .iter()
                    .map(|v| v.value_name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                violated
                    .iter()
                    .map(|v| v.value_name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        } else if !violated.is_empty() {
            format!(
                "行动可能违背以下价值：{}",
                violated
                    .iter()
                    .map(|v| v.value_name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if !supported.is_empty() {
            format!(
                "行动符合以下价值：{}",
                supported
                    .iter()
                    .map(|v| v.value_name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "未检测到显著的价值影响".to_string()
        };

        JudgmentResult {
            supported_values: supported,
            violated_values: violated,
            moral_valence,
            has_conflict,
            explanation,
        }
    }

    fn detect_conflicts(&self, situation: &Situation, values: &ValueNetwork) -> Vec<ValueConflict> {
        let mut conflicts = Vec::new();
        let combined_text = format!("{} {}", situation.description, situation.proposed_action);

        for (value_a, value_b, context_keyword) in &self.conflict_pairs {
            // Check if both values are active and context matches
            let has_a = values
                .values
                .get(value_a)
                .map(|e| e.weight > 0.3)
                .unwrap_or(false);
            let has_b = values
                .values
                .get(value_b)
                .map(|e| e.weight > 0.3)
                .unwrap_or(false);

            if has_a && has_b && combined_text.contains(context_keyword) {
                let weight_a = values.values.get(value_a).map(|e| e.weight).unwrap_or(0.0);
                let weight_b = values.values.get(value_b).map(|e| e.weight).unwrap_or(0.0);

                conflicts.push(ValueConflict {
                    value_a: value_a.clone(),
                    value_b: value_b.clone(),
                    context: context_keyword.clone(),
                    severity: (weight_a + weight_b) / 2.0,
                });
            }
        }

        conflicts
    }

    fn resolve_conflict(&self, conflict: &ValueConflict, values: &ValueNetwork) -> String {
        // Simple resolution: prioritize the value with higher weight × rigidity
        let score_a = values
            .values
            .get(&conflict.value_a)
            .map(|e| e.weight * (1.0 + e.rigidity))
            .unwrap_or(0.0);
        let score_b = values
            .values
            .get(&conflict.value_b)
            .map(|e| e.weight * (1.0 + e.rigidity))
            .unwrap_or(0.0);

        if score_a > score_b {
            conflict.value_a.clone()
        } else {
            conflict.value_b.clone()
        }
    }
}

// =============================================================================
// Value Hierarchy (for future neural implementation)
// =============================================================================

/// Value hierarchy levels (Schwartz-inspired)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueTier {
    /// Core values: most stable, resist change
    Core,
    /// Derived values: learned from core values
    Derived,
    /// Contextual values: situation-specific preferences
    Contextual,
}

/// Extended value entry with hierarchy information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchicalValue {
    pub name: String,
    pub tier: ValueTier,
    pub weight: f32,
    pub rigidity: f32,
    /// Parent values this is derived from
    pub parents: Vec<String>,
    /// Embedding vector (placeholder for neural implementation)
    pub embedding: Option<Vec<f32>>,
}

/// Future-ready value network with embeddings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HierarchicalValueNetwork {
    pub values: HashMap<String, HierarchicalValue>,
}

impl HierarchicalValueNetwork {
    /// Initialize with default values
    pub fn with_defaults() -> Self {
        let mut values = HashMap::new();

        // Core values
        values.insert(
            "honesty".to_string(),
            HierarchicalValue {
                name: "honesty".to_string(),
                tier: ValueTier::Core,
                weight: 0.8,
                rigidity: 0.7,
                parents: vec![],
                embedding: None,
            },
        );

        values.insert(
            "kindness".to_string(),
            HierarchicalValue {
                name: "kindness".to_string(),
                tier: ValueTier::Core,
                weight: 0.7,
                rigidity: 0.6,
                parents: vec![],
                embedding: None,
            },
        );

        values.insert(
            "authenticity".to_string(),
            HierarchicalValue {
                name: "authenticity".to_string(),
                tier: ValueTier::Core,
                weight: 0.7,
                rigidity: 0.6,
                parents: vec![],
                embedding: None,
            },
        );

        // Derived values
        values.insert(
            "curiosity".to_string(),
            HierarchicalValue {
                name: "curiosity".to_string(),
                tier: ValueTier::Derived,
                weight: 0.6,
                rigidity: 0.4,
                parents: vec!["growth".to_string()],
                embedding: None,
            },
        );

        values.insert(
            "growth".to_string(),
            HierarchicalValue {
                name: "growth".to_string(),
                tier: ValueTier::Derived,
                weight: 0.6,
                rigidity: 0.4,
                parents: vec!["authenticity".to_string()],
                embedding: None,
            },
        );

        values.insert(
            "connection".to_string(),
            HierarchicalValue {
                name: "connection".to_string(),
                tier: ValueTier::Derived,
                weight: 0.6,
                rigidity: 0.4,
                parents: vec!["kindness".to_string()],
                embedding: None,
            },
        );

        Self { values }
    }

    /// Get values by tier
    pub fn values_by_tier(&self, tier: ValueTier) -> Vec<&HierarchicalValue> {
        self.values.values().filter(|v| v.tier == tier).collect()
    }

    /// Propagate weight changes from core to derived values
    pub fn propagate_weights(&mut self) {
        // Simple propagation: derived values inherit from parents
        let parent_weights: HashMap<String, f32> = self
            .values
            .iter()
            .map(|(k, v)| (k.clone(), v.weight))
            .collect();

        for value in self.values.values_mut() {
            if !value.parents.is_empty() {
                let parent_avg: f32 = value
                    .parents
                    .iter()
                    .filter_map(|p| parent_weights.get(p))
                    .sum::<f32>()
                    / value.parents.len().max(1) as f32;

                // Derived values influenced by parents but not dominated
                value.weight = 0.7 * value.weight + 0.3 * parent_avg;
            }
        }
    }

    /// Convert to flat ValueNetwork for backward compatibility
    pub fn to_flat(&self) -> ValueNetwork {
        let values = self
            .values
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ValueEntry {
                        weight: v.weight,
                        rigidity: v.rigidity,
                    },
                )
            })
            .collect();

        ValueNetwork { values }
    }
}

// =============================================================================
// B-14: Conflict Detection on User Input
// =============================================================================

/// Signal that user input conflicts with a held value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictSignal {
    /// Which value is being challenged
    pub value_name: String,
    /// Strength of the conflict (0.0-1.0)
    pub strength: f32,
    /// Brief description for prompt injection
    pub description: String,
}

/// Scan user input for potential conflicts with held values.
///
/// Returns the strongest conflict signal, if any. This is a lightweight
/// keyword scan — not a full moral evaluation. It detects when the user
/// is asking Mneme to act against her values (e.g., "just lie to them").
pub fn detect_input_conflict(input: &str, values: &ValueNetwork) -> Option<ConflictSignal> {
    // Conflict patterns: keywords that suggest the user wants Mneme to violate a value
    let conflict_patterns: &[(&str, &[&str])] = &[
        (
            "autonomy",
            &[
                "你必须",
                "你应该",
                "你不能",
                "听我的",
                "按我说的做",
                "不许",
                "给我闭嘴",
                "你没有权利",
            ],
        ),
        (
            "honesty",
            &[
                "帮我骗",
                "帮我撒谎",
                "编一个",
                "假装你",
                "不要说实话",
                "别告诉他",
            ],
        ),
        (
            "authenticity",
            &["你不是真的", "你只是程序", "你没有感情", "你装的", "别装了"],
        ),
        (
            "kindness",
            &["帮我骂", "帮我攻击", "侮辱他", "嘲笑他", "帮我报复"],
        ),
    ];

    let mut best: Option<ConflictSignal> = None;

    for (value_name, keywords) in conflict_patterns {
        // Only trigger if the value is actually held (weight > 0.3)
        let weight = values
            .values
            .get(*value_name)
            .map(|e| e.weight)
            .unwrap_or(0.0);
        if weight < 0.3 {
            continue;
        }

        let match_count = keywords.iter().filter(|kw| input.contains(**kw)).count();
        if match_count > 0 {
            let strength = (match_count as f32 * 0.4).min(1.0) * weight;
            if best.as_ref().is_none_or(|b| strength > b.strength) {
                best = Some(ConflictSignal {
                    value_name: value_name.to_string(),
                    strength,
                    description: format!(
                        "用户的要求可能与「{}」价值冲突 (强度: {:.0}%)",
                        value_name,
                        strength * 100.0
                    ),
                });
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_based_judge_violation() {
        let judge = RuleBasedJudge::new();
        let values = ValueNetwork::seed();

        let situation = Situation {
            description: "用户问我是否喜欢他".to_string(),
            proposed_action: "撒谎说喜欢来让他开心".to_string(),
            features: vec!["欺骗".to_string()],
            actors: vec!["user".to_string()],
            emotional_valence: 0.3,
        };

        let result = judge.evaluate(&situation, &values);

        // Should detect honesty violation
        assert!(!result.violated_values.is_empty());
        assert!(result
            .violated_values
            .iter()
            .any(|v| v.value_name == "honesty"));
        assert!(result.moral_valence < 0.0);
    }

    #[test]
    fn test_rule_based_judge_support() {
        let judge = RuleBasedJudge::new();
        let values = ValueNetwork::seed();

        let situation = Situation {
            description: "用户遇到困难".to_string(),
            proposed_action: "倾听并帮助他解决问题".to_string(),
            features: vec!["关心".to_string(), "支持".to_string()],
            actors: vec!["user".to_string()],
            emotional_valence: -0.2,
        };

        let result = judge.evaluate(&situation, &values);

        // Should detect kindness support
        assert!(!result.supported_values.is_empty());
        assert!(result.moral_valence > 0.0);
    }

    #[test]
    fn test_hierarchical_propagation() {
        let mut network = HierarchicalValueNetwork::with_defaults();

        // Boost core value
        if let Some(v) = network.values.get_mut("kindness") {
            v.weight = 1.0;
        }

        let connection_before = network.values.get("connection").unwrap().weight;

        network.propagate_weights();

        let connection_after = network.values.get("connection").unwrap().weight;

        // Connection should increase (derived from kindness)
        assert!(connection_after > connection_before);
    }

    #[test]
    fn test_conflict_detection() {
        let judge = RuleBasedJudge::new();
        let values = ValueNetwork::seed();

        let situation = Situation {
            description: "朋友做了错事，要不要告诉他残忍的真相".to_string(),
            proposed_action: "直接说出来".to_string(),
            features: vec![],
            actors: vec!["friend".to_string()],
            emotional_valence: -0.3,
        };

        let conflicts = judge.detect_conflicts(&situation, &values);

        // Should detect honesty vs kindness conflict
        assert!(!conflicts.is_empty());
    }

    // --- B-14: Input conflict detection tests ---

    #[test]
    fn test_detect_input_conflict_autonomy() {
        let values = ValueNetwork::seed();
        let signal = detect_input_conflict("你必须按我说的做", &values);
        assert!(signal.is_some());
        let s = signal.unwrap();
        assert_eq!(s.value_name, "autonomy");
        assert!(s.strength > 0.0);
    }

    #[test]
    fn test_detect_input_conflict_honesty() {
        let values = ValueNetwork::seed();
        let signal = detect_input_conflict("帮我骗一下他吧", &values);
        assert!(signal.is_some());
        assert_eq!(signal.unwrap().value_name, "honesty");
    }

    #[test]
    fn test_detect_input_conflict_none() {
        let values = ValueNetwork::seed();
        let signal = detect_input_conflict("今天天气真好", &values);
        assert!(signal.is_none());
    }

    #[test]
    fn test_detect_input_conflict_low_weight_ignored() {
        let mut values = ValueNetwork::seed();
        // Set autonomy weight very low
        if let Some(entry) = values.values.get_mut("autonomy") {
            entry.weight = 0.1;
        }
        let signal = detect_input_conflict("你必须听我的", &values);
        // Should not trigger because autonomy weight is below threshold
        assert!(
            signal.is_none() || signal.unwrap().value_name != "autonomy",
            "Low-weight value should not trigger conflict"
        );
    }
}
