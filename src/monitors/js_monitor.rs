use anyhow::{Result, anyhow};
use log::{debug, info};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::monitors::{Monitor, Change};

/// API data monitor (formerly JavaScript data monitor)
/// Now directly monitors API endpoints for data changes
pub struct JsMonitor {
    /// Target URL (API endpoint)
    url: String,
    /// JSON path or key to monitor (using dot notation, e.g., "data.price")
    selector: String,
    /// Last detected value
    last_value: Option<String>,
    /// Check interval (seconds)
    interval_secs: u64,
    /// HTTP client
    client: Client,
}

impl JsMonitor {
    /// Create a new API monitor
    pub fn new(url: &str, selector: &str, interval_secs: u64) -> Self {
        // Create HTTP client with timeout
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
            
        Self {
            url: url.to_string(),
            selector: selector.to_string(),
            last_value: None,
            interval_secs,
            client,
        }
    }
    
    /// Extract value from JSON response using selector path
    fn extract_value(&self, json_data: &Value, selector: &str) -> Result<String> {
        // Split the selector by dots to navigate the JSON structure
        let parts: Vec<&str> = selector.split('.').collect();
        
        // Start with the root JSON value
        let mut current = json_data;
        
        // Navigate through each part of the selector
        for part in parts {
            // Try to access the current part as a field
            current = match current.get(part) {
                Some(value) => value,
                None => return Err(anyhow!("Failed to find '{}' in JSON response", part)),
            };
        }
        
        // Convert the final value to a string
        Ok(match current {
            Value::String(s) => s.clone(),
            _ => current.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl Monitor for JsMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        debug!("API monitor check: {}", self.url);
        
        // Send HTTP request to API endpoint
        let response = self.client.get(&self.url)
            .send()
            .await
            .map_err(|e| anyhow!("API request failed: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API request failed, status code: {}", status));
        }
        
        // Parse JSON response
        let json: Value = response.json()
            .await
            .map_err(|e| anyhow!("Failed to parse JSON response: {}", e))?;
            
        // Extract value based on selector
        let value = self.extract_value(&json, &self.selector)?;
        
        debug!("API value retrieved: {}", value);
        
        // Check if there's a change
        if let Some(last_value) = &self.last_value {
            if *last_value != value {
                // Change detected
                info!("API data changed ({}): {} -> {}", self.url, last_value, value);
                
                // Update last value
                let old_value = last_value.clone();
                self.last_value = Some(value.clone());
                
                return Ok(Some(Change {
                    message: format!("API data changed: {}", self.url),
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
    
    fn get_name(&self) -> String {
        format!("JavaScript monitor for {}", self.url)
    }
} 