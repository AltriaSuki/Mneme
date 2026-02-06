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
