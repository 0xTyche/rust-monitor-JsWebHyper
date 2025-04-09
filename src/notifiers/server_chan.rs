use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, CONTENT_LENGTH};
use serde_json::Value;
use serde_urlencoded;
use regex::Regex;

use crate::notifiers::Notifier;

/// ServerChan notification service, used to send notifications to WeChat
pub struct ServerChanNotifier {
    /// ServerChan's SCKEY
    key: String,
    /// HTTP client
    client: Client,
}

impl ServerChanNotifier {
    /// Create a new ServerChan notification service
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            client: Client::new(),
        }
    }
    
    /// Send notification using the sc_send method provided by FangTang
    async fn sc_send(&self, text: &str, desp: &str) -> Result<String> {
        let params = [("text", text), ("desp", desp)];
        let post_data = serde_urlencoded::to_string(params)
            .map_err(|e| anyhow!("Failed to encode request parameters: {}", e))?;
            
        // Extract the numeric part from the key using regex
        let url = if self.key.starts_with("sctp") {
            let re = Regex::new(r"sctp(\d+)t")
                .map_err(|e| anyhow!("Failed to parse regex: {}", e))?;
                
            if let Some(captures) = re.captures(&self.key) {
                let num = &captures[1]; // Extract the numeric part captured by regex
                format!("https://{}.push.ft07.com/send/{}.send", num, self.key)
            } else {
                return Err(anyhow!("ServerChan key format is incorrect"));
            }
        } else {
            format!("https://sctapi.ftqq.com/{}.send", self.key)
        };
        
        debug!("Sending notification to: {}", url);
        
        let res = self.client.post(&url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(CONTENT_LENGTH, post_data.len() as u64)
            .body(post_data)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send notification request: {}", e))?;
            
        let status = res.status();
        if !status.is_success() {
            return Err(anyhow!("Failed to send notification request, status code: {}", status));
        }
            
        let data = res.text().await
            .map_err(|e| anyhow!("Failed to read response content: {}", e))?;
            
        debug!("Notification response: {}", data);
        
        Ok(data)
    }
}

impl Notifier for ServerChanNotifier {
    async fn send(&self, title: &str, content: &str) -> Result<()> {
        if self.key.is_empty() {
            error!("ServerChan key not set, cannot send notification");
            return Err(anyhow!("ServerChan key not set"));
        }
        
        debug!("Sending ServerChan notification: {}", title);
        
        // Send notification using the sc_send method provided by FangTang
        let response = self.sc_send(title, content).await?;
        
        // Parse response
        let data: Value = serde_json::from_str(&response)
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;
            
        // Check if successful
        let code = data["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            let message = data["message"].as_str().unwrap_or("Unknown error");
            return Err(anyhow!("Failed to send notification: {}", message));
        }
        
        debug!("Notification sent successfully");
        
        Ok(())
    }
} 