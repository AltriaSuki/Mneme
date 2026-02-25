//! Simple keyword-based Chinese/English sentiment analysis.
//!
//! Shared across crates to avoid duplicating word lists.
//! In production, this should be replaced with an ML model.

const POSITIVE: &[&str] = &[
    "开心", "高兴", "喜欢", "喜爱", "真棒", "很好", "太好了", "不错",
    "谢谢", "感谢", "感激", "哈哈", "有趣", "有意思", "厉害", "优秀",
    "快乐", "幸福", "满意", "舒服", "温暖", "期待",
    "😊", "❤️", "👍", "🎉",
];

const NEGATIVE: &[&str] = &[
    "难过", "伤心", "讨厌", "痛恨", "糟糕", "很差", "太差",
    "烦躁", "烦恼", "生气", "愤怒", "失望", "焦虑", "害怕",
    "无聊", "孤独", "沮丧", "崩溃", "绝望", "痛苦",
    "😢", "😡", "💔", "😞",
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
        let (v, i) = analyze_sentiment("明天出门");
        assert!((v - 0.0).abs() < 0.01);
        assert!((i - 0.1).abs() < 0.01);
        // "天气" should no longer false-match (old "气" keyword removed)
        let (v2, _) = analyze_sentiment("今天天气不错");
        assert!(v2 >= 0.0, "天气 should not trigger negative sentiment");
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
