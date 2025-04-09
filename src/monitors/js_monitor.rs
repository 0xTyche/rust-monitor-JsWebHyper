use anyhow::{Result, anyhow};
use log::{debug, info};
use headless_chrome::Browser;
use serde_json::Value;

use crate::monitors::{Monitor, Change};

/// JavaScript data monitor
pub struct JsMonitor {
    /// Target URL
    url: String,
    /// JavaScript selector
    selector: String,
    /// Last detected value
    last_value: Option<String>,
    /// Check interval (seconds)
    interval_secs: u64,
}

impl JsMonitor {
    /// Create a new JavaScript monitor
    pub fn new(url: &str, selector: &str, interval_secs: u64) -> Self {
        Self {
            url: url.to_string(),
            selector: selector.to_string(),
            last_value: None,
            interval_secs,
        }
    }
}

impl Monitor for JsMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        debug!("JS monitor check: {}", self.url);
        
        // Create browser instance
        let browser = Browser::default()
            .map_err(|e| anyhow!("Failed to create browser instance: {}", e))?;
        
        // Create new tab
        let tab = browser.wait_for_initial_tab()
            .map_err(|e| anyhow!("Failed to get initial tab: {}", e))?;
        
        // Navigate to target URL
        tab.navigate_to(&self.url)
            .map_err(|e| anyhow!("Failed to navigate to target URL: {}", e))?;
        
        // Wait for page to load
        tab.wait_until_navigated()
            .map_err(|e| anyhow!("Failed to wait for page load: {}", e))?;
        
        // Execute JavaScript to get data
        let eval_result = tab.evaluate(&self.selector, false)
            .map_err(|e| anyhow!("Failed to execute JavaScript: {}", e))?;
            
        // Get value and convert to string
        let value = match eval_result.value {
            Some(Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => return Err(anyhow!("Cannot get JavaScript execution result")),
        };
        
        debug!("JavaScript value retrieved: {}", value);
        
        // Check if there's a change
        if let Some(last_value) = &self.last_value {
            if *last_value != value {
                // Change detected
                info!("JS data changed ({}): {} -> {}", self.url, last_value, value);
                
                // Update last value
                let old_value = last_value.clone();
                self.last_value = Some(value.clone());
                
                return Ok(Some(Change {
                    message: format!("JavaScript data changed: {}", self.url),
                    details: format!("Selector: {}\nOld value: {}\nNew value: {}", self.selector, old_value, value),
                }));
            } else {
                // No change
                debug!("No change");
                return Ok(None);
            }
        } else {
            // First check, record initial value
            debug!("First check, recording initial value: {}", value);
            self.last_value = Some(value);
            return Ok(None);
        }
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
} 