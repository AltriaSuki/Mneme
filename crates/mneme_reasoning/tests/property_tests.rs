//! Property-based tests for mneme_reasoning sanitize_chat_output.
//!
//! Verifies that the output sanitizer is idempotent, never panics on arbitrary
//! input, and correctly strips markdown/roleplay artifacts.

use proptest::prelude::*;
use mneme_reasoning::engine::sanitize_chat_output;

// ============================================================================
// Sanitize Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// **Idempotency**: sanitize(sanitize(x)) == sanitize(x) for all x.
    /// This is critical — applying the filter twice should be a no-op.
    #[test]
    fn sanitize_idempotent(s in "\\PC{0,500}") {
        let once = sanitize_chat_output(&s);
        let twice = sanitize_chat_output(&once);
        prop_assert_eq!(&once, &twice,
            "Not idempotent!\nInput:  {:?}\nOnce:   {:?}\nTwice:  {:?}", s, once, twice);
    }

    /// **Never panics** on arbitrary Unicode strings, including edge cases.
    #[test]
    fn sanitize_never_panics(s in "\\PC{0,1000}") {
        let _ = sanitize_chat_output(&s);
    }

    /// **No markdown headers in output**: after sanitization, no line starts with `# `.
    #[test]
    fn sanitize_removes_headers(s in "\\PC{0,300}") {
        let output = sanitize_chat_output(&s);
        for line in output.lines() {
            prop_assert!(
                !line.starts_with("# ") && !line.starts_with("## ") && !line.starts_with("### "),
                "Header survived: {:?} in output of {:?}", line, s
            );
        }
    }

    /// **No markdown bullets in output**: no line starts with `- ` or `* `.
    #[test]
    fn sanitize_removes_bullets(s in "\\PC{0,300}") {
        let output = sanitize_chat_output(&s);
        for line in output.lines() {
            prop_assert!(
                !line.starts_with("- ") && !line.starts_with("* "),
                "Bullet survived: {:?} in output of {:?}", line, s
            );
        }
    }

    /// **No bold markers in output**: `**text**` pattern is removed.
    #[test]
    fn sanitize_removes_bold(
        prefix in "[^*]{0,20}",
        inner in "[^*]{1,20}",
        suffix in "[^*]{0,20}",
    ) {
        let input = format!("{}**{}**{}", prefix, inner, suffix);
        let output = sanitize_chat_output(&input);
        prop_assert!(
            !output.contains("**"),
            "Bold survived in output: {:?} → {:?}", input, output
        );
    }

    /// **No triple newlines in output**: at most 2 consecutive newlines.
    #[test]
    fn sanitize_no_triple_newlines(s in "\\PC{0,500}") {
        let output = sanitize_chat_output(&s);
        prop_assert!(
            !output.contains("\n\n\n"),
            "Triple newline found in output: {:?}", output
        );
    }

    /// **Pure Chinese text is preserved**: if input has no markdown artifacts,
    /// output should be similar to input (just trimmed).
    /// Note: we exclude `*`, `-`, `#` from the input since those are intentionally
    /// stripped by the sanitizer (they're markdown artifacts in chat context).
    #[test]
    fn sanitize_preserves_pure_chinese(s in "[\\p{Han}]{1,100}") {
        let output = sanitize_chat_output(&s);
        let trimmed_input = s.trim();
        if !trimmed_input.is_empty() {
            prop_assert!(
                !output.is_empty(),
                "Pure Chinese input was emptied: {:?} → {:?}", s, output
            );
        }
    }
}

// ============================================================================
// Specific regression patterns
// ============================================================================

#[test]
fn sanitize_single_asterisk_roleplay() {
    assert_eq!(sanitize_chat_output("*叹气*你好"), "叹气你好");
    assert_eq!(sanitize_chat_output("*开心地笑了*"), "开心地笑了");
}

#[test]
fn sanitize_double_asterisk_bold() {
    assert_eq!(sanitize_chat_output("**重要**的事"), "重要的事");
}

#[test]
fn sanitize_mixed_markdown() {
    let input = "# 标题\n\n**重点**内容\n\n- 列表1\n- 列表2\n\n*动作*结尾";
    let output = sanitize_chat_output(input);
    assert!(!output.contains('#'));
    assert!(!output.contains('*'));
    assert!(!output.starts_with('-'));
}

#[test]
fn sanitize_empty_input() {
    assert_eq!(sanitize_chat_output(""), "");
}

#[test]
fn sanitize_only_whitespace() {
    assert_eq!(sanitize_chat_output("   \n\n  "), "");
}
