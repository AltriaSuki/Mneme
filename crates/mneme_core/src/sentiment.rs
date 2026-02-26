//! Simple keyword-based Chinese/English sentiment analysis.
//!
//! Shared across crates to avoid duplicating word lists.
//! In production, this should be replaced with an ML model.

// Multi-char positive keywords (safe from substring false-positives)
const POSITIVE: &[&str] = &[
    "开心", "高兴", "喜欢", "喜爱", "真棒", "很好", "太好了", "不错",
    "谢谢", "感谢", "感激", "哈哈", "有趣", "有意思", "厉害", "优秀",
    "快乐", "幸福", "满意", "舒服", "温暖", "期待", "兴奋", "激动",
    "太棒", "真好", "好棒", "好开心", "好高兴", "心情好", "感动",
    "惊喜", "庆祝", "恭喜", "成功", "顺利", "完美", "精彩", "美好",
    "可爱", "欣慰", "自豪", "骄傲", "振奋", "愉快", "畅快", "爽快",
    "赞", "棒", "爽",
    "😊", "❤️", "👍", "🎉", "😄", "🥰", "✨",
];

// Negation prefixes — if these appear right before a positive word,
// the positive match is cancelled (e.g. "不好" negates "好").
const NEGATION: &[&str] = &["不", "没", "别", "莫", "未"];

// Multi-char negative keywords
const NEGATIVE: &[&str] = &[
    "难过", "伤心", "讨厌", "痛恨", "糟糕", "很差", "太差",
    "烦躁", "烦恼", "生气", "愤怒", "失望", "焦虑", "害怕",
    "无聊", "孤独", "沮丧", "崩溃", "绝望", "痛苦", "悲伤",
    "郁闷", "压抑", "无助", "恐惧", "厌恶", "后悔", "懊悔",
    "心烦", "心累", "受不了", "撑不住", "扛不住", "熬不住",
    "裁员", "失业", "分手", "离婚", "去世", "病了", "出事",
    "😢", "😡", "💔", "😞", "😭", "🥺",
];

const INTENSE: &[&str] = &[
    "非常", "特别", "超级", "极其", "太", "真的", "实在",
    "!", "！", "?!", "？！", "!!", "！！",
];

/// Analyze text for emotional valence and intensity.
///
/// Returns `(valence, intensity)` where:
/// - `valence` is in `[-1.0, 1.0]` (negative to positive)
/// - `intensity` is in `[0.1, 1.0]`
pub fn analyze_sentiment(text: &str) -> (f32, f32) {
    let mut pos = 0_f32;
    let mut neg = 0_f32;

    // Count positive keywords, checking for negation
    for kw in POSITIVE {
        if let Some(idx) = text.find(kw) {
            // Check if preceded by a negation prefix → flip to negative
            let prefix = &text[..idx];
            let negated = NEGATION.iter().any(|neg| prefix.ends_with(neg));
            if negated {
                neg += 1.0;
            } else {
                pos += 1.0;
            }
        }
    }

    // Count negative keywords, checking for negation (double negative → positive)
    for kw in NEGATIVE {
        if let Some(idx) = text.find(kw) {
            let prefix = &text[..idx];
            let negated = NEGATION.iter().any(|neg| prefix.ends_with(neg));
            if negated {
                pos += 0.5; // double negative is weakly positive
            } else {
                neg += 1.0;
            }
        }
    }

    let int = INTENSE.iter().filter(|w| text.contains(*w)).count() as f32;

    // Valence: tanh-like scaling so single strong signal reaches ±0.7
    let raw = pos - neg;
    let valence = if raw.abs() < 0.01 {
        0.0
    } else {
        // tanh(raw * 0.8) gives: 1 match → ±0.66, 2 → ±0.92, 3 → ±0.98
        (raw * 0.8).tanh()
    };

    // Intensity: any emotional signal raises baseline; intensifiers boost further
    let signal = pos + neg;
    let intensity = if signal < 0.01 {
        0.1 + (int * 0.15).min(0.3) // intensifiers alone give mild intensity
    } else {
        (0.3 + signal * 0.15 + int * 0.1).clamp(0.1, 1.0)
    };

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
        let (v2, _) = analyze_sentiment("今天天气不错");
        assert!(v2 > 0.0, "不错 should be positive");
    }

    #[test]
    fn test_positive_text() {
        let (v, _) = analyze_sentiment("我很开心，谢谢你");
        assert!(v > 0.3, "two positive words should give strong valence, got {v}");
    }

    #[test]
    fn test_negative_text() {
        let (v, _) = analyze_sentiment("我很难过，讨厌这样");
        assert!(v < -0.3, "two negative words should give strong negative, got {v}");
    }

    #[test]
    fn test_intense_text() {
        let (_, i1) = analyze_sentiment("好");
        let (_, i2) = analyze_sentiment("非常好！");
        assert!(i2 > i1, "intensifiers should boost: {i1} vs {i2}");
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

    #[test]
    fn test_offer_good_news() {
        // This was the original failing case
        let (v, i) = analyze_sentiment("我今天心情特别好，刚拿到了一个很棒的offer！");
        assert!(v > 0.3, "offer good news should be positive, got {v}");
        assert!(i > 0.3, "should have notable intensity, got {i}");
    }

    #[test]
    fn test_layoff_bad_news() {
        let (v, i) = analyze_sentiment("我被裁员了，感觉很绝望，不知道该怎么办");
        assert!(v < -0.3, "layoff should be strongly negative, got {v}");
        assert!(i > 0.3, "should have notable intensity, got {i}");
    }

    #[test]
    fn test_negation_flips_positive() {
        let (v, _) = analyze_sentiment("今天不开心");
        assert!(v < 0.0, "negated positive should be negative, got {v}");
    }

    #[test]
    fn test_double_negation() {
        let (v, _) = analyze_sentiment("不难过");
        assert!(v > 0.0, "double negation should be weakly positive, got {v}");
    }
}
