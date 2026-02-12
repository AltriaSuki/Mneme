//! Simple keyword-based Chinese/English sentiment analysis.
//!
//! Shared across crates to avoid duplicating word lists.
//! In production, this should be replaced with an ML model.

const POSITIVE: &[&str] = &[
    "开心", "高兴", "喜欢", "爱", "棒", "好", "谢谢", "感谢", "哈哈", "😊", "❤️", "👍",
];

const NEGATIVE: &[&str] = &[
    "难过", "伤心", "讨厌", "恨", "糟糕", "差", "烦", "气", "怒", "😢", "😡", "💔",
];

const INTENSE: &[&str] = &[
    "非常", "特别", "超级", "极其", "太", "!", "！", "?!", "？！",
];

/// Analyze text for emotional valence and intensity.
///
/// Returns `(valence, intensity)` where:
/// - `valence` is in `[-1.0, 1.0]` (negative to positive)
/// - `intensity` is in `[0.1, 1.0]`
pub fn analyze_sentiment(text: &str) -> (f32, f32) {
    let pos = POSITIVE.iter().filter(|w| text.contains(*w)).count() as f32;
    let neg = NEGATIVE.iter().filter(|w| text.contains(*w)).count() as f32;
    let int = INTENSE.iter().filter(|w| text.contains(*w)).count() as f32;

    let valence = (pos - neg) / (pos + neg + 1.0);
    let intensity = ((pos + neg + int) / 5.0).clamp(0.1, 1.0);

    (valence, intensity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neutral_text() {
        // "天气" contains "气" (negative keyword), so use a string with no keyword substrings
        let (v, i) = analyze_sentiment("明天出门");
        assert!((v - 0.0).abs() < 0.01);
        assert!((i - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_positive_text() {
        let (v, _) = analyze_sentiment("我很开心，谢谢你");
        assert!(v > 0.0);
    }

    #[test]
    fn test_negative_text() {
        let (v, _) = analyze_sentiment("我很难过，讨厌这样");
        assert!(v < 0.0);
    }

    #[test]
    fn test_intense_text() {
        let (_, i1) = analyze_sentiment("好");
        let (_, i2) = analyze_sentiment("非常好！");
        assert!(i2 > i1);
    }

    #[test]
    fn test_emoji_sentiment() {
        let (v, _) = analyze_sentiment("😊👍");
        assert!(v > 0.0);
    }

    #[test]
    fn test_empty_text() {
        let (v, i) = analyze_sentiment("");
        assert!((v - 0.0).abs() < 0.01);
        assert!((i - 0.1).abs() < 0.01);
    }
}
