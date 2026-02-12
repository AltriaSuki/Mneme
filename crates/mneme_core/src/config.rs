use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

// ============================================================================
// Top-level config
// ============================================================================

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MnemeConfig {
    pub llm: LlmConfig,
    pub safety: SafetyConfig,
    pub token_budget: TokenBudgetConfig,
    pub organism: OrganismDefaults,
    pub onebot: Option<OneBotConfig>,
}

impl MnemeConfig {
    /// Load config from a TOML file, falling back to defaults for missing fields.
    /// After loading, env var overrides are applied.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        let mut config: MnemeConfig =
            toml::from_str(&content).with_context(|| "Failed to parse TOML config")?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Try to load from path; if file doesn't exist, return defaults with env overrides.
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        match Self::load(path) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::info!("Config file not found or invalid ({}), using defaults", e);
                let mut cfg = Self::default();
                cfg.apply_env_overrides();
                cfg
            }
        }
    }

    /// Apply environment variable overrides on top of file-based config.
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("LLM_PROVIDER") {
            self.llm.provider = v;
        }
        if let Ok(v) = std::env::var("ANTHROPIC_MODEL") {
            self.llm.model = v;
        }
        if let Ok(v) = std::env::var("LLM_BASE_URL") {
            self.llm.base_url = Some(v);
        }
        if let Ok(v) = std::env::var("LLM_MAX_TOKENS") {
            if let Ok(n) = v.parse() {
                self.llm.max_tokens = n;
            }
        }
        if let Ok(v) = std::env::var("LLM_TEMPERATURE") {
            if let Ok(n) = v.parse() {
                self.llm.temperature = n;
            }
        }
        if let Ok(v) = std::env::var("LLM_CONTEXT_BUDGET") {
            if let Ok(n) = v.parse() {
                self.llm.context_budget_chars = n;
            }
        }
        // OneBot env overrides
        if let Ok(url) = std::env::var("ONEBOT_WS_URL") {
            let token = std::env::var("ONEBOT_ACCESS_TOKEN").ok();
            self.onebot = Some(OneBotConfig {
                ws_url: url,
                access_token: token,
            });
        }
    }
}

// ============================================================================
// Sub-configs
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Context budget in characters for the system prompt assembly pipeline.
    /// Should roughly match the model's context window (~4 chars per token).
    /// Default: 32000 (~8k tokens).
    pub context_budget_chars: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-4-5-sonnet-20250929".to_string(),
            base_url: None,
            max_tokens: 4096,
            temperature: 0.7,
            context_budget_chars: 32_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    pub tier: CapabilityTier,
    pub allowed_paths: Vec<PathBuf>,
    pub blocked_commands: Vec<String>,
    pub network_whitelist: Vec<String>,
    pub require_confirmation: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            tier: CapabilityTier::Restricted,
            allowed_paths: vec![],
            blocked_commands: default_blocked_commands(),
            network_whitelist: vec![],
            require_confirmation: true,
        }
    }
}

fn default_blocked_commands() -> Vec<String> {
    vec![
        "rm -rf /".to_string(),
        "rm -rf /*".to_string(),
        "mkfs".to_string(),
        "dd if=".to_string(),
        ":(){ :|:& };:".to_string(),
        "sudo rm".to_string(),
        "> /dev/sda".to_string(),
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityTier {
    /// Read-only: ls, cat, git status, curl GET
    ReadOnly,
    /// Restricted writes within allowed_paths
    #[default]
    Restricted,
    /// Full access (must be explicitly enabled)
    Full,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TokenBudgetConfig {
    pub daily_limit: Option<u64>,
    pub monthly_limit: Option<u64>,
    pub warning_threshold: f32,
    pub degradation_strategy: DegradationStrategy,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            daily_limit: None,
            monthly_limit: None,
            warning_threshold: 0.8,
            degradation_strategy: DegradationStrategy::HardStop,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradationStrategy {
    #[default]
    HardStop,
    Degrade {
        max_tokens_cap: u32,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OrganismDefaults {
    pub db_path: String,
    pub persona_dir: String,
    pub tick_interval_secs: u64,
    pub trigger_interval_secs: u64,
}

impl Default for OrganismDefaults {
    fn default() -> Self {
        Self {
            db_path: "mneme.db".to_string(),
            persona_dir: "persona".to_string(),
            tick_interval_secs: 10,
            trigger_interval_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OneBotConfig {
    pub ws_url: String,
    pub access_token: Option<String>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = MnemeConfig::default();
        assert_eq!(cfg.llm.provider, "anthropic");
        assert_eq!(cfg.llm.max_tokens, 4096);
        assert_eq!(cfg.safety.tier, CapabilityTier::Restricted);
        assert!(cfg.onebot.is_none());
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = r#"
[llm]
provider = "deepseek"
model = "deepseek-chat"
"#;
        let cfg: MnemeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.llm.provider, "deepseek");
        assert_eq!(cfg.llm.model, "deepseek-chat");
        // Defaults for unspecified fields
        assert_eq!(cfg.llm.max_tokens, 4096);
        assert_eq!(cfg.safety.tier, CapabilityTier::Restricted);
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r#"
[llm]
provider = "openai"
model = "gpt-4"
base_url = "https://api.openai.com/v1"
max_tokens = 8192
temperature = 0.9

[safety]
tier = "read_only"
allowed_paths = ["/tmp"]
blocked_commands = ["rm -rf /"]
network_whitelist = ["api.openai.com"]
require_confirmation = false

[token_budget]
daily_limit = 100000
monthly_limit = 3000000
warning_threshold = 0.75

[organism]
db_path = "data/mneme.db"
persona_dir = "my_persona"
tick_interval_secs = 5
trigger_interval_secs = 30

[onebot]
ws_url = "ws://localhost:8080"
access_token = "secret"
"#;
        let cfg: MnemeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.llm.provider, "openai");
        assert_eq!(cfg.llm.max_tokens, 8192);
        assert_eq!(cfg.safety.tier, CapabilityTier::ReadOnly);
        assert!(!cfg.safety.require_confirmation);
        assert_eq!(cfg.token_budget.daily_limit, Some(100000));
        assert_eq!(cfg.organism.tick_interval_secs, 5);
        let onebot = cfg.onebot.unwrap();
        assert_eq!(onebot.ws_url, "ws://localhost:8080");
        assert_eq!(onebot.access_token, Some("secret".to_string()));
    }

    #[test]
    fn test_parse_degrade_strategy() {
        let toml_str = r#"
[token_budget]
daily_limit = 50000
degradation_strategy = { degrade = { max_tokens_cap = 1024 } }
"#;
        let cfg: MnemeConfig = toml::from_str(toml_str).unwrap();
        match cfg.token_budget.degradation_strategy {
            DegradationStrategy::Degrade { max_tokens_cap } => {
                assert_eq!(max_tokens_cap, 1024);
            }
            _ => panic!("Expected Degrade strategy"),
        }
    }

    #[test]
    fn test_env_overrides_and_defaults() {
        // Part 1: env overrides
        std::env::set_var("LLM_PROVIDER", "openai");
        std::env::set_var("ANTHROPIC_MODEL", "gpt-4o");

        let mut cfg = MnemeConfig::default();
        cfg.apply_env_overrides();

        assert_eq!(cfg.llm.provider, "openai");
        assert_eq!(cfg.llm.model, "gpt-4o");

        // Clean up env vars before testing defaults
        std::env::remove_var("LLM_PROVIDER");
        std::env::remove_var("ANTHROPIC_MODEL");

        // Part 2: nonexistent path returns defaults (no env interference)
        let cfg = MnemeConfig::load_or_default("/nonexistent/path.toml");
        assert_eq!(cfg.llm.provider, "anthropic");
    }
}
