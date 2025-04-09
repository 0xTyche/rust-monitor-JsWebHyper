use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::monitors::{Monitor, Change};

/// Static webpage monitor, used to monitor webpage content changes
pub struct StaticMonitor {
    /// Webpage URL to monitor
    url: String,
    /// HTML selector
    selector: String,
    /// Monitoring interval (seconds)
    interval_secs: u64,
    /// Last detected content
    last_content: Option<String>,
    /// HTTP client
    client: Client,
}

impl StaticMonitor {
    /// Create a new static webpage monitor
    pub fn new(url: &str, selector: &str, interval_secs: u64) -> Self {
        // Create HTTP client with timeout
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
            
        Self {
            url: url.to_string(),
            selector: selector.to_string(),
            interval_secs,
            last_content: None,
            client,
        }
    }
    
    /// Get content of specified element from webpage
    async fn get_content(&self) -> Result<String> {
        debug!("Getting webpage content: {} - {}", self.url, self.selector);
        
        // Send HTTP request to get webpage content
        let response = self.client.get(&self.url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to get webpage content: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP request failed, status code: {}", status));
        }
        
        let html = response.text()
            .await
            .map_err(|e| anyhow!("Failed to read response content: {}", e))?;
        
        // If selector is empty or just whitespace, return the entire page content
        let selector = self.selector.trim();
        if selector.is_empty() || selector == "*" || selector == "body" {
            debug!("Using entire page content: {} bytes", html.len());
            return Ok(html);
        }
            
        // Parse HTML
        let document = Html::parse_document(&html);
        
        // Parse selector
        let selector = Selector::parse(&self.selector)
            .map_err(|e| anyhow!("Failed to parse selector: {}", e))?;
            
        // Get content of matching elements
        let content = document.select(&selector)
            .map(|element| element.inner_html())
            .collect::<Vec<String>>()
            .join("\n");
            
        if content.is_empty() {
            return Err(anyhow!("No matching elements found"));
        }
        
        debug!("Content retrieved: {} bytes", content.len());
        
        Ok(content)
    }
}

#[async_trait::async_trait]
impl Monitor for StaticMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        match self.get_content().await {
            Ok(current_content) => {
                // Check if content has changed
                if let Some(last_content) = &self.last_content {
                    if *last_content != current_content {
                        // Content has changed
                        let change = Change {
                            message: format!("Webpage content changed: {}", self.url),
                            details: format!(
                                "Selector: {}\nContent length: {} -> {} bytes", 
                                self.selector, 
                                last_content.len(), 
                                current_content.len()
                            ),
                        };
                        
                        // Update last content
                        self.last_content = Some(current_content);
                        
                        return Ok(Some(change));
                    }
                } else {
                    // First check, don't trigger change notification
                    debug!("First time getting content: {} bytes", current_content.len());
                    self.last_content = Some(current_content);
                }
                
                Ok(None)
            }
            Err(e) => {
                error!("Failed to get webpage content: {}", e);
                Err(anyhow!("Failed to get webpage content: {}", e))
            }
        }
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
    
    fn get_name(&self) -> String {
        format!("Static webpage monitor for {}", self.url)
    }
} 