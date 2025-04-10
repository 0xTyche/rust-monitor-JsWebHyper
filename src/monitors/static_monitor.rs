use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use std::time::Duration;

use crate::monitors::{Monitor, Change};

/// Static webpage monitor, used to monitor webpage content changes
pub struct StaticMonitor {
    /// Webpage URL to monitor
    url: String,
    /// Monitoring interval (seconds)
    interval_secs: u64,
    /// Last detected content
    last_content: Option<String>,
    /// HTTP client
    client: Client,
}

impl StaticMonitor {
    /// Create a new static webpage monitor
    pub fn new(url: &str, _selector: &str, interval_secs: u64) -> Self {
        // Create HTTP client with timeout
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
            
        Self {
            url: url.to_string(),
            interval_secs,
            last_content: None,
            client,
        }
    }
    
    /// Get content of webpage
    async fn get_content(&self) -> Result<String> {
        debug!("Getting entire webpage content: {}", self.url);
        
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
        
        debug!("Full webpage content retrieved: {} bytes", html.len());
        
        Ok(html)
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
                                "Content length: {} -> {} bytes", 
                                last_content.len(), 
                                current_content.len()
                            ),
                        };
                        
                        // Update last content
                        self.last_content = Some(current_content);
                        
                        return Ok(Some(change));
                    }
                } else {
                    // First check, create change notification with initial content
                    debug!("First time getting content: {} bytes", current_content.len());
                    
                    // Create change for initial content
                    let change = Change {
                        message: format!("Initial webpage content: {}", self.url),
                        details: format!("Initial content length: {} bytes", current_content.len()),
                    };
                    
                    // Store the content
                    self.last_content = Some(current_content);
                    
                    return Ok(Some(change));
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