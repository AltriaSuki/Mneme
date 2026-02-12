//! Affect Model based on Russell's Circumplex Model of Emotion
//!
//! Instead of discrete emotion labels (Happy/Sad/Angry), we use a continuous
//! 2D coordinate system: Valence × Arousal. This allows for nuanced, mixed emotions.

use crate::state::deserialize_safe_f32;
use serde::{Deserialize, Serialize};

/// Russell's Circumplex Model: 2D emotional state
///
/// This replaces the discrete `Emotion` enum with a continuous representation.
/// Any emotion can be expressed as a point in this 2D space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affect {
    /// Valence: positive/negative (-1.0 to 1.0)
    /// - Positive: joy, contentment, excitement
    /// - Negative: sadness, fear, anger
    #[serde(deserialize_with = "deserialize_safe_f32")]
    pub valence: f32,

    /// Arousal: calm/activated (0.0 to 1.0)
    /// - High: excited, anxious, angry
    /// - Low: calm, relaxed, depressed
    #[serde(deserialize_with = "deserialize_safe_f32")]
    pub arousal: f32,
}

impl Default for Affect {
    fn default() -> Self {
        Self {
            valence: 0.0, // Neutral
            arousal: 0.3, // Slightly calm baseline
        }
    }
}

impl Affect {
    pub fn new(valence: f32, arousal: f32) -> Self {
        Self {
            valence: valence.clamp(-1.0, 1.0),
            arousal: arousal.clamp(0.0, 1.0),
        }
    }

    /// Create affect from polar coordinates (angle in radians, intensity 0-1)
    pub fn from_polar(angle: f32, intensity: f32) -> Self {
        let intensity = intensity.clamp(0.0, 1.0);
        Self {
            valence: angle.cos() * intensity,
            arousal: (angle.sin() * intensity + 1.0) / 2.0, // Map to 0-1
        }
    }

    /// Get the emotional intensity (distance from neutral origin)
    pub fn intensity(&self) -> f32 {
        (self.valence.powi(2) + (self.arousal * 2.0 - 1.0).powi(2)).sqrt()
    }

    /// Interpolate between two affects
    pub fn lerp(&self, other: &Affect, t: f32) -> Affect {
        let t = t.clamp(0.0, 1.0);
        Affect {
            valence: self.valence + (other.valence - self.valence) * t,
            arousal: self.arousal + (other.arousal - self.arousal) * t,
        }
    }

    /// Common affect presets for convenience
    pub fn joy() -> Self {
        Self::new(0.8, 0.6)
    }
    pub fn excitement() -> Self {
        Self::new(0.7, 0.9)
    }
    pub fn contentment() -> Self {
        Self::new(0.6, 0.2)
    }
    pub fn serenity() -> Self {
        Self::new(0.4, 0.1)
    }

    pub fn sadness() -> Self {
        Self::new(-0.7, 0.2)
    }
    pub fn depression() -> Self {
        Self::new(-0.8, 0.1)
    }
    pub fn anxiety() -> Self {
        Self::new(-0.5, 0.8)
    }
    pub fn fear() -> Self {
        Self::new(-0.7, 0.9)
    }
    pub fn anger() -> Self {
        Self::new(-0.8, 0.9)
    }
    pub fn frustration() -> Self {
        Self::new(-0.5, 0.6)
    }

    pub fn surprise() -> Self {
        Self::new(0.1, 0.9)
    }
    pub fn boredom() -> Self {
        Self::new(-0.3, 0.1)
    }
    pub fn neutral() -> Self {
        Self::default()
    }

    /// Get the closest discrete emotion label (for backward compatibility and TTS)
    pub fn to_discrete_label(&self) -> &'static str {
        // Quadrant-based classification with intensity threshold
        let intensity = self.intensity();

        if intensity < 0.2 {
            return "neutral";
        }

        match (self.valence >= 0.0, self.arousal >= 0.5) {
            (true, true) => {
                if self.arousal > 0.7 {
                    "excited"
                } else {
                    "happy"
                }
            }
            (true, false) => "calm",
            (false, true) => {
                if self.valence < -0.5 {
                    "angry"
                } else {
                    "anxious"
                }
            }
            (false, false) => "sad",
        }
    }

    /// Describe the affect in natural language (for LLM context injection)
    pub fn describe(&self) -> String {
        let intensity = self.intensity();

        let intensity_word = if intensity < 0.2 {
            "平静"
        } else if intensity < 0.4 {
            "略微"
        } else if intensity < 0.6 {
            "比较"
        } else if intensity < 0.8 {
            "相当"
        } else {
            "非常"
        };

        let emotion_word = match (self.valence >= 0.0, self.arousal >= 0.5) {
            (true, true) => {
                if self.valence > 0.5 && self.arousal > 0.7 {
                    "兴奋愉悦"
                } else if self.valence > 0.5 {
                    "开心"
                } else {
                    "有些期待"
                }
            }
            (true, false) => {
                if self.valence > 0.5 {
                    "满足平和"
                } else {
                    "放松"
                }
            }
            (false, true) => {
                if self.valence < -0.5 && self.arousal > 0.7 {
                    "烦躁不安"
                } else if self.valence < -0.3 {
                    "焦虑"
                } else {
                    "有些紧张"
                }
            }
            (false, false) => {
                if self.valence < -0.5 {
                    "低落沮丧"
                } else {
                    "有点闷闷不乐"
                }
            }
        };

        if intensity < 0.2 {
            "情绪平稳，没有明显波动".to_string()
        } else {
            format!("{}{}的", intensity_word, emotion_word)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affect_presets() {
        let joy = Affect::joy();
        assert!(joy.valence > 0.5);
        assert!(joy.arousal > 0.4);

        let anger = Affect::anger();
        assert!(anger.valence < -0.5);
        assert!(anger.arousal > 0.7);
    }

    #[test]
    fn test_intensity() {
        let neutral = Affect::neutral();
        assert!(neutral.intensity() < 0.5);

        let extreme = Affect::new(1.0, 1.0);
        assert!(extreme.intensity() > 0.9);
    }

    #[test]
    fn test_describe() {
        let joy = Affect::joy();
        let desc = joy.describe();
        assert!(desc.contains("开心") || desc.contains("愉悦"));
    }

    #[test]
    fn test_from_polar_zero_intensity() {
        let affect = Affect::from_polar(0.0, 0.0);
        assert!((affect.valence - 0.0).abs() < 1e-6);
        assert!((affect.arousal - 0.5).abs() < 1e-6); // (sin(0)*0 + 1) / 2 = 0.5
    }

    #[test]
    fn test_from_polar_full_positive() {
        // angle=0 → cos=1, sin=0 → valence=1.0, arousal=0.5
        let affect = Affect::from_polar(0.0, 1.0);
        assert!((affect.valence - 1.0).abs() < 1e-6);
        assert!((affect.arousal - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_from_polar_clamps_intensity() {
        let affect = Affect::from_polar(0.0, 5.0);
        assert!(affect.valence <= 1.0);
        assert!(affect.arousal >= 0.0 && affect.arousal <= 1.0);
    }

    #[test]
    fn test_new_clamps_values() {
        let affect = Affect::new(5.0, -3.0);
        assert_eq!(affect.valence, 1.0);
        assert_eq!(affect.arousal, 0.0);

        let affect2 = Affect::new(-5.0, 10.0);
        assert_eq!(affect2.valence, -1.0);
        assert_eq!(affect2.arousal, 1.0);
    }

    #[test]
    fn test_lerp_endpoints() {
        let a = Affect::joy();
        let b = Affect::sadness();

        // t=0 → returns a
        let at_zero = a.lerp(&b, 0.0);
        assert!((at_zero.valence - a.valence).abs() < 1e-6);
        assert!((at_zero.arousal - a.arousal).abs() < 1e-6);

        // t=1 → returns b
        let at_one = a.lerp(&b, 1.0);
        assert!((at_one.valence - b.valence).abs() < 1e-6);
        assert!((at_one.arousal - b.arousal).abs() < 1e-6);
    }

    #[test]
    fn test_lerp_midpoint() {
        let a = Affect::new(0.0, 0.0);
        let b = Affect::new(1.0, 1.0);
        let mid = a.lerp(&b, 0.5);
        assert!((mid.valence - 0.5).abs() < 1e-6);
        assert!((mid.arousal - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_lerp_clamps_t() {
        let a = Affect::joy();
        let b = Affect::sadness();

        // t < 0 should clamp to 0
        let clamped = a.lerp(&b, -1.0);
        assert!((clamped.valence - a.valence).abs() < 1e-6);

        // t > 1 should clamp to 1
        let clamped = a.lerp(&b, 2.0);
        assert!((clamped.valence - b.valence).abs() < 1e-6);
    }

    #[test]
    fn test_to_discrete_label_quadrants() {
        assert_eq!(Affect::joy().to_discrete_label(), "happy");
        assert_eq!(Affect::excitement().to_discrete_label(), "excited");
        assert_eq!(Affect::contentment().to_discrete_label(), "calm");
        assert_eq!(Affect::sadness().to_discrete_label(), "sad");
        assert_eq!(Affect::anger().to_discrete_label(), "angry");
        assert_eq!(Affect::anxiety().to_discrete_label(), "anxious");
    }

    #[test]
    fn test_to_discrete_label_neutral() {
        // arousal=0.5 → intensity = sqrt(0 + 0) = 0 → neutral
        let neutral = Affect::new(0.0, 0.5);
        assert_eq!(neutral.to_discrete_label(), "neutral");
    }

    #[test]
    fn test_describe_neutral() {
        let neutral = Affect::new(0.0, 0.5);
        let desc = neutral.describe();
        assert!(
            desc.contains("平稳"),
            "Neutral should describe as 平稳, got: {}",
            desc
        );
    }

    #[test]
    fn test_describe_negative() {
        let sad = Affect::sadness();
        let desc = sad.describe();
        assert!(
            desc.contains("低落") || desc.contains("闷闷不乐"),
            "Sadness should describe as 低落/闷闷不乐, got: {}",
            desc
        );
    }

    #[test]
    fn test_safe_f32_json_roundtrip_affect() {
        let affect = Affect::new(0.6, 0.8);
        let json = serde_json::to_string(&affect).unwrap();
        let restored: Affect = serde_json::from_str(&json).unwrap();
        assert!((restored.valence - 0.6).abs() < 1e-6);
        assert!((restored.arousal - 0.8).abs() < 1e-6);
    }
}
