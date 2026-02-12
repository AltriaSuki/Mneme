use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BrowserAction {
    Goto { url: String },
    Click { selector: String },
    Type { selector: String, text: String },
    Screenshot,
    GetHtml,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_action_serde_roundtrip() {
        let actions = vec![
            BrowserAction::Goto {
                url: "https://example.com".to_string(),
            },
            BrowserAction::Click {
                selector: "#btn".to_string(),
            },
            BrowserAction::Type {
                selector: "input".to_string(),
                text: "hello".to_string(),
            },
            BrowserAction::Screenshot,
            BrowserAction::GetHtml,
        ];

        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let roundtripped: BrowserAction = serde_json::from_str(&json).unwrap();
            // Compare debug representations
            assert_eq!(format!("{:?}", action), format!("{:?}", roundtripped));
        }
    }

    #[test]
    fn test_browser_action_goto() {
        let json = r#"{"action":"goto","url":"https://example.com"}"#;
        let action: BrowserAction = serde_json::from_str(json).unwrap();
        match action {
            BrowserAction::Goto { url } => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected Goto variant"),
        }
    }

    #[test]
    fn test_browser_action_type() {
        let json = r##"{"action":"type","selector":"#input","text":"world"}"##;
        let action: BrowserAction = serde_json::from_str(json).unwrap();
        match action {
            BrowserAction::Type { selector, text } => {
                assert_eq!(selector, "#input");
                assert_eq!(text, "world");
            }
            _ => panic!("Expected Type variant"),
        }
    }
}
