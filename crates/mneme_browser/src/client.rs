use anyhow::Result;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::sync::Arc;

pub struct BrowserClient {
    browser: Browser,
    current_tab: Option<Arc<Tab>>,
}

impl BrowserClient {
    pub fn new(headless: bool) -> Result<Self> {
        let options = LaunchOptions {
            headless,
            ..Default::default()
        };
        let browser = Browser::new(options)?;
        Ok(Self {
            browser,
            current_tab: None,
        })
    }

    pub fn launch(&mut self) -> Result<()> {
        let tab = self.browser.wait_for_initial_tab()?;
        self.current_tab = Some(tab);
        Ok(())
    }

    pub fn goto(&mut self, url: &str) -> Result<()> {
        if let Some(tab) = &self.current_tab {
            tab.navigate_to(url)?;
            tab.wait_until_navigated()?;
        }
        Ok(())
    }

    pub fn screenshot(&self) -> Result<Vec<u8>> {
        if let Some(tab) = &self.current_tab {
            use headless_chrome::protocol::cdp::Page;
            let png_data = tab.capture_screenshot(
                Page::CaptureScreenshotFormatOption::Png, 
                None, 
                None, 
                true
            )?;
            Ok(png_data)
        } else {
            Err(anyhow::anyhow!("No active tab"))
        }
    }
    
    pub fn get_title(&self) -> Result<String> {
        if let Some(tab) = &self.current_tab {
            tab.get_title()
        } else {
            Err(anyhow::anyhow!("No active tab"))
        }
    }

    pub fn click(&mut self, selector: &str) -> Result<()> {
        if let Some(tab) = &self.current_tab {
            tab.wait_for_element(selector)?.click()?;
            Ok(())
        } else {
             Err(anyhow::anyhow!("No active tab"))
        }
    }

    pub fn type_text(&mut self, selector: &str, text: &str) -> Result<()> {
        if let Some(tab) = &self.current_tab {
            tab.wait_for_element(selector)?.click()?;
            tab.type_str(text)?;
            Ok(())
        } else {
             Err(anyhow::anyhow!("No active tab"))
        }
    }

    pub fn get_html(&self) -> Result<String> {
        if let Some(tab) = &self.current_tab {
             // Basic content retrieval
             // In future, we might want to return a simplified structure or accessibility tree
             tab.get_content()
        } else {
             Err(anyhow::anyhow!("No active tab"))
        }
    }

    pub fn execute_action(&mut self, action: super::action::BrowserAction) -> Result<String> {
        use super::action::BrowserAction::*;
        match action {
            Goto { url } => {
                self.goto(&url)?;
                Ok(format!("Navigated to {}", url))
            },
            Click { selector } => {
                self.click(&selector)?;
                Ok(format!("Clicked element '{}'", selector))
            },
            Type { selector, text } => {
                self.type_text(&selector, &text)?;
                Ok(format!("Typed '{}' into '{}'", text, selector))
            },
            Screenshot => {
                let data = self.screenshot()?;
                // We return a placeholder string because transferring binary via this string return is messy
                // Ideally, we save it to a file or return a base64 string
                // For now, let's just say we took it.
                Ok(format!("Screenshot taken ({} bytes)", data.len()))
            },
            GetHtml => {
                let html = self.get_html()?;
                Ok(format!("HTML Content ({} chars):\n{}", html.len(), html))
            }
        }
    }
}
