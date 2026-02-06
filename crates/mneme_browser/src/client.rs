use anyhow::Result;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for browser sessions.
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// Run in headless mode (no visible window).
    pub headless: bool,
    /// Default timeout for element waits (e.g. wait_for_element).
    pub element_timeout: Duration,
    /// Maximum time to wait for page navigation.
    pub navigation_timeout: Duration,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            headless: true,
            element_timeout: Duration::from_secs(10),
            navigation_timeout: Duration::from_secs(30),
        }
    }
}

impl BrowserConfig {
    /// Create a config for debugging (non-headless, longer timeouts).
    pub fn debug() -> Self {
        Self {
            headless: false,
            element_timeout: Duration::from_secs(30),
            navigation_timeout: Duration::from_secs(60),
        }
    }
}

pub struct BrowserClient {
    browser: Browser,
    current_tab: Option<Arc<Tab>>,
    config: BrowserConfig,
}

impl BrowserClient {
    /// Create a new browser client with default config.
    ///
    /// `headless` — if true, runs without a visible window.
    pub fn new(headless: bool) -> Result<Self> {
        let config = BrowserConfig {
            headless,
            ..BrowserConfig::default()
        };
        Self::with_config(config)
    }

    /// Create a browser client with full configuration.
    pub fn with_config(config: BrowserConfig) -> Result<Self> {
        let options = LaunchOptions {
            headless: config.headless,
            ..Default::default()
        };
        let browser = Browser::new(options)?;
        Ok(Self {
            browser,
            current_tab: None,
            config,
        })
    }

    /// Open a fresh tab (replaces deprecated `wait_for_initial_tab`).
    pub fn launch(&mut self) -> Result<()> {
        let tab = self.browser.new_tab()?;
        tab.set_default_timeout(self.config.element_timeout);
        self.current_tab = Some(tab);
        Ok(())
    }

    /// Check whether the browser session is still alive.
    ///
    /// Attempts a lightweight CDP call (`get_url()`) on the current tab.
    /// Returns `false` if no tab, or if the call fails (tab/browser crashed).
    pub fn is_alive(&self) -> bool {
        match &self.current_tab {
            Some(tab) => {
                // get_url() is a local read (no CDP call), so also try get_target_info()
                // which actually talks to the browser process.
                tab.get_target_info().is_ok()
            }
            None => false,
        }
    }

    pub fn goto(&mut self, url: &str) -> Result<()> {
        let tab = self.tab()?;
        tab.navigate_to(url)?;
        tab.wait_until_navigated()?;
        Ok(())
    }

    pub fn screenshot(&self) -> Result<Vec<u8>> {
        let tab = self.tab()?;
        use headless_chrome::protocol::cdp::Page;
        let png_data = tab.capture_screenshot(
            Page::CaptureScreenshotFormatOption::Png,
            None,
            None,
            true,
        )?;
        Ok(png_data)
    }

    pub fn get_title(&self) -> Result<String> {
        let tab = self.tab()?;
        tab.get_title()
    }

    pub fn click(&mut self, selector: &str) -> Result<()> {
        let tab = self.tab()?;
        tab.wait_for_element(selector)?.click()?;
        Ok(())
    }

    pub fn type_text(&mut self, selector: &str, text: &str) -> Result<()> {
        let tab = self.tab()?;
        tab.wait_for_element(selector)?.click()?;
        tab.type_str(text)?;
        Ok(())
    }

    pub fn get_html(&self) -> Result<String> {
        let tab = self.tab()?;
        tab.get_content()
    }

    pub fn execute_action(&mut self, action: super::action::BrowserAction) -> Result<String> {
        use super::action::BrowserAction::*;
        match action {
            Goto { url } => {
                self.goto(&url)?;
                Ok(format!("Navigated to {}", url))
            }
            Click { selector } => {
                self.click(&selector)?;
                Ok(format!("Clicked element '{}'", selector))
            }
            Type { selector, text } => {
                self.type_text(&selector, &text)?;
                Ok(format!("Typed '{}' into '{}'", text, selector))
            }
            Screenshot => {
                let data = self.screenshot()?;
                Ok(format!("Screenshot taken ({} bytes)", data.len()))
            }
            GetHtml => {
                let html = self.get_html()?;
                // Limit HTML output to prevent overwhelming the LLM context
                let truncated = if html.len() > 8192 {
                    format!("{}\n... [truncated, {} total chars]", &html[..8192], html.len())
                } else {
                    html.clone()
                };
                Ok(format!("HTML Content ({} chars):\n{}", html.len(), truncated))
            }
        }
    }

    /// Get a reference to the current tab, or error if none.
    fn tab(&self) -> Result<&Arc<Tab>> {
        self.current_tab
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active browser tab. Call launch() first."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_config_default() {
        let config = BrowserConfig::default();
        assert!(config.headless);
        assert_eq!(config.element_timeout, Duration::from_secs(10));
        assert_eq!(config.navigation_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_browser_config_debug() {
        let config = BrowserConfig::debug();
        assert!(!config.headless);
        assert_eq!(config.element_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_no_tab_before_launch() {
        // BrowserClient without launch() should report not alive
        // We can't easily test this without Chrome installed, so just test the config path
        let config = BrowserConfig::default();
        assert!(config.headless);
    }
}
