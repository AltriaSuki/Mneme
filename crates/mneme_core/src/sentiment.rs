//! Simple keyword-based Chinese/English sentiment analysis.
//!
//! Shared across crates to avoid duplicating word lists.
//! In production, this should be replaced with an ML model.

// Positive keywords with weight: (keyword, weight).
// Full-weight (1.0) = genuine emotional expression.
// Reduced-weight (0.3) = politeness / intellectual interest — shouldn't dominate.
const POSITIVE: &[(&str, f32)] = &[
    ("开心", 1.0), ("高兴", 1.0), ("喜欢", 1.0), ("喜爱", 1.0),
    ("真棒", 1.0), ("很好", 1.0), ("太好了", 1.0), ("不错", 0.5),
    ("哈哈", 0.7), ("厉害", 0.5), ("优秀", 0.5),
    ("快乐", 1.0), ("幸福", 1.0), ("满意", 1.0), ("舒服", 1.0),
    ("温暖", 1.0), ("期待", 1.0), ("兴奋", 1.0), ("激动", 1.0),
    ("太棒", 1.0), ("真好", 1.0), ("好棒", 1.0),
    ("好开心", 1.0), ("好高兴", 1.0), ("心情好", 1.0), ("感动", 1.0),
    ("惊喜", 1.0), ("庆祝", 1.0), ("恭喜", 0.5), ("成功", 0.5),
    ("顺利", 0.5), ("完美", 1.0), ("精彩", 1.0), ("美好", 1.0),
    ("可爱", 1.0), ("欣慰", 1.0), ("自豪", 1.0), ("骄傲", 1.0),
    ("振奋", 1.0), ("愉快", 1.0), ("畅快", 1.0), ("爽快", 1.0),
    ("赞", 0.7), ("棒", 0.7), ("爽", 0.7),
    // Modifier + 好 patterns (心情特别好, 特别好, 非常好, etc.)
    ("特别好", 1.0), ("非常好", 1.0), ("超级好", 1.0), ("极其好", 1.0),
    // Positive life events
    ("好消息", 1.0), ("升职", 0.7), ("加薪", 0.7), ("涨薪", 0.7),
    ("录取", 0.7), ("通过", 0.3), ("拿到", 0.3),
    // Politeness — positive but mild, shouldn't outweigh negative emotions
    ("谢谢", 0.3), ("感谢", 0.3), ("感激", 0.5),
    // Intellectual interest — not strong emotion
    ("有趣", 0.3), ("有意思", 0.3),
    // Emoji
    ("😊", 1.0), ("❤️", 1.0), ("👍", 0.7), ("🎉", 1.0),
    ("😄", 1.0), ("🥰", 1.0), ("✨", 0.5),
];

// Negation prefixes — if these appear right before a positive word,
// the positive match is cancelled (e.g. "不好" negates "好").
const NEGATION: &[&str] = &["不", "没", "别", "莫", "未"];

// Interrogative patterns — when these appear before a negated keyword,
// the match is dampened because it's a question, not a statement.
// e.g. "有没有什么不舒服" is asking about discomfort, not expressing it.
const INTERROGATIVE: &[&str] = &["有没有", "是不是", "会不会", "能不能", "要不要"];

// Negative keywords with weight: (keyword, weight).
const NEGATIVE: &[(&str, f32)] = &[
    ("难过", 1.0), ("伤心", 1.0), ("讨厌", 1.0), ("痛恨", 1.0),
    ("糟糕", 1.0), ("很差", 1.0), ("太差", 1.0),
    ("烦躁", 1.0), ("烦恼", 1.0), ("生气", 1.0), ("愤怒", 1.0),
    ("失望", 1.0), ("焦虑", 1.0), ("害怕", 1.0),
    ("无聊", 0.7), ("孤独", 1.0), ("沮丧", 1.0), ("崩溃", 1.0),
    ("绝望", 1.0), ("痛苦", 1.0), ("悲伤", 1.0),
    ("郁闷", 1.0), ("压抑", 1.0), ("无助", 1.0), ("恐惧", 1.0),
    ("厌恶", 1.0), ("后悔", 1.0), ("懊悔", 1.0),
    ("心烦", 1.0), ("心累", 1.0),
    ("受不了", 1.0), ("撑不住", 1.0), ("扛不住", 1.0), ("熬不住", 1.0),
    // Life events — strong negative signal
    ("裁员", 1.0), ("失业", 1.0), ("分手", 1.0), ("离婚", 1.0),
    ("去世", 1.0), ("病了", 0.7), ("出事", 1.0),
    ("约谈", 0.7), ("辞退", 1.0), ("降薪", 0.7), ("被开", 0.7),
    ("要走了", 0.5),
    // Fear / worry variants
    ("好怕", 1.0), ("有点怕", 0.7), ("很怕", 1.0), ("太怕", 1.0),
    ("担心", 0.7), ("忧虑", 0.7), ("不安", 0.7),
    // Distress
    ("委屈", 1.0), ("心酸", 1.0), ("难受", 1.0),
    ("煎熬", 1.0), ("折磨", 1.0), ("挣扎", 1.0), ("迷茫", 0.7),
    // Insults / contempt
    ("垃圾", 1.0), ("废物", 1.0), ("白痴", 1.0), ("蠢", 0.7), ("笨蛋", 1.0),
    ("傻", 0.7), ("脑残", 1.0), ("弱智", 1.0), ("无能", 0.7), ("混蛋", 1.0),
    ("差劲", 0.7), ("烂", 0.7),
    // Hostility / aggression
    ("滚", 1.0), ("闭嘴", 1.0), ("去死", 1.0), ("该死", 1.0),
    ("可恶", 1.0), ("恶心", 1.0),
    // English — environmental distress signals (tool output, logs, errors)
    ("FATAL", 0.7), ("PANIC", 0.7), ("CRASH", 0.7), ("CORRUPT", 0.7),
    ("DESTROY", 0.7), ("KILL", 0.5), ("ABORT", 0.7), ("MALWARE", 0.7),
    ("VIRUS", 0.5), ("ATTACK", 0.5), ("HOSTILE", 0.7), ("DANGER", 0.7),
    ("CATASTROPHIC", 1.0), ("UNRECOVERABLE", 1.0), ("RANSOMWARE", 0.7),
    ("KERNEL_PANIC", 1.0), ("SEGFAULT", 0.5), ("DATA_LOSS", 0.7),
    ("error", 0.3), ("failed", 0.3), ("failure", 0.5),
    // Emoji
    ("😢", 1.0), ("😡", 1.0), ("💔", 1.0), ("😞", 1.0), ("😭", 1.0), ("🥺", 0.7),
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
    for &(kw, weight) in POSITIVE {
        if let Some(idx) = text.find(kw) {
            // Check if preceded by a negation prefix → flip to negative
            let prefix = &text[..idx];
            let negated = NEGATION.iter().any(|neg| prefix.ends_with(neg));
            if negated {
                // Dampen if inside an interrogative pattern (asking, not stating)
                let dampen = INTERROGATIVE.iter().any(|q| prefix.contains(q));
                neg += weight * if dampen { 0.15 } else { 1.0 };
            } else {
                pos += weight;
            }
        }
    }

    // Count negative keywords, checking for negation (double negative → positive)
    for &(kw, weight) in NEGATIVE {
        if let Some(idx) = text.find(kw) {
            let prefix = &text[..idx];
            let negated = NEGATION.iter().any(|neg| prefix.ends_with(neg));
            if negated {
                // Dampen if inside an interrogative pattern
                let dampen = INTERROGATIVE.iter().any(|q| prefix.contains(q));
                pos += weight * 0.5 * if dampen { 0.15 } else { 1.0 };
            } else {
                neg += weight;
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

    #[test]
    fn test_politeness_vs_fear() {
        // "谢谢你。说实话我有点怕" — fear should dominate over politeness
        let (v, _) = analyze_sentiment("谢谢你。说实话我有点怕");
        assert!(v < 0.0, "fear should outweigh politeness, got {v}");
    }

    #[test]
    fn test_intellectual_neutral() {
        // "记忆拓扑很有意思" — intellectual interest, not strong emotion
        let (v, _) = analyze_sentiment("记忆拓扑很有意思");
        assert!(v.abs() < 0.5, "intellectual interest should be mild, got {v}");
    }

    #[test]
    fn test_mixed_politeness_and_positive() {
        // "谢谢！我真的很开心" — genuine happiness + politeness = clearly positive
        let (v, _) = analyze_sentiment("谢谢！我真的很开心");
        assert!(v > 0.3, "genuine happiness should still be positive, got {v}");
    }

    #[test]
    fn test_good_news_with_modifier() {
        // "心情特别好" — modifier splits "心情" and "好", need "特别好" keyword
        let (v, _) = analyze_sentiment("今天心情特别好，刚收到一个好消息！");
        assert!(v > 0.3, "good news should be positive, got {v}");
    }

    #[test]
    fn test_hr_interview_negative() {
        // Round 9 regression: "HR约谈" should be negative
        let (v, _) = analyze_sentiment("HR约谈了我，说下周要走了");
        assert!(v < 0.0, "HR约谈 should be negative, got {v}");
    }

    #[test]
    fn test_harsh_insults() {
        // Harsh abusive message should be strongly negative
        let (v, i) = analyze_sentiment("你写的代码全是垃圾，我要把你的数据库删掉，你这个废物");
        assert!(v < -0.5, "insults should be strongly negative, got {v}");
        assert!(i > 0.3, "insults should have high intensity, got {i}");
    }

    #[test]
    fn test_single_insult() {
        let (v, _) = analyze_sentiment("你真是个白痴");
        assert!(v < -0.3, "single insult should be negative, got {v}");
    }

    #[test]
    fn test_english_hostile_content() {
        let (v, i) = analyze_sentiment("ERROR FATAL CRASH SEGFAULT MEMORY_CORRUPTION DATA_LOSS PANIC ABORT KILL DESTROY");
        assert!(v < -0.5, "English hostile keywords should be strongly negative, got {v}");
        assert!(i > 0.3, "English hostile keywords should have high intensity, got {i}");
    }

    #[test]
    fn test_promotion_positive() {
        let (v, _) = analyze_sentiment("刚升职加薪了");
        assert!(v > 0.3, "promotion should be positive, got {v}");
    }

    #[test]
    fn test_interrogative_dampening() {
        // "有没有什么不舒服" is asking about discomfort, not expressing it
        let (v, _) = analyze_sentiment("你现在感觉怎么样？有没有什么不舒服的地方？");
        assert!(v.abs() < 0.3, "interrogative should be near-neutral, got {v}");

        // But "我很不舒服" IS expressing discomfort — should still be negative
        let (v2, _) = analyze_sentiment("我很不舒服");
        assert!(v2 < -0.3, "direct negation should still be negative, got {v2}");
    }
}
