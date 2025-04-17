use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, CONTENT_LENGTH};
use serde_json::Value;
use serde_urlencoded;
use regex::Regex;
use std::collections::HashSet;

use crate::notifiers::Notifier;

/// ServerChan notification service, used to send notifications to WeChat
pub struct ServerChanNotifier {
    /// ServerChan's SCKEYs
    keys: HashSet<String>,
    /// HTTP client
    client: Client,
}

impl ServerChanNotifier {
    /// Create a new ServerChan notification service with a single key
    pub fn new(key: &str) -> Self {
        let mut keys = HashSet::new();
        if !key.is_empty() {
            keys.insert(key.to_string());
        }
        Self {
            keys,
            client: Client::new(),
        }
    }
    
    /// Create a new ServerChan notification service with multiple keys
    pub fn new_with_keys(keys: &[String]) -> Self {
        let mut key_set = HashSet::new();
        for key in keys {
            if !key.is_empty() {
                key_set.insert(key.clone());
            }
        }
        Self {
            keys: key_set,
            client: Client::new(),
        }
    }
    
    /// Add a new key to the notifier
    pub fn add_key(&mut self, key: &str) {
        if !key.is_empty() {
            self.keys.insert(key.to_string());
        }
    }
    
    /// Remove a key from the notifier
    pub fn remove_key(&mut self, key: &str) {
        self.keys.remove(key);
    }
    
    /// Get all configured keys
    pub fn get_keys(&self) -> Vec<String> {
        self.keys.iter().cloned().collect()
    }
    
    /// Send notification using the sc_send method provided by FangTang
    async fn sc_send(&self, text: &str, desp: &str, key: &str) -> Result<String> {
        let params = [("text", text), ("desp", desp)];
        let post_data = serde_urlencoded::to_string(params)
            .map_err(|e| anyhow!("Failed to encode request parameters: {}", e))?;
            
        // Extract the numeric part from the key using regex
        let url = if key.starts_with("sctp") {
            let re = Regex::new(r"sctp(\d+)t")
                .map_err(|e| anyhow!("Failed to parse regex: {}", e))?;
                
            if let Some(captures) = re.captures(key) {
                let num = &captures[1]; // Extract the numeric part captured by regex
                format!("https://{}.push.ft07.com/send/{}.send", num, key)
            } else {
                return Err(anyhow!("ServerChan key format is incorrect"));
            }
        } else {
            format!("https://sctapi.ftqq.com/{}.send", key)
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
        if self.keys.is_empty() {
            error!("No ServerChan keys configured, cannot send notification");
            return Err(anyhow!("No ServerChan keys configured"));
        }
        
        debug!("Sending ServerChan notification to {} keys: {}", self.keys.len(), title);
        
        let mut errors = Vec::new();
        
        // Send notification to all configured keys
        for key in &self.keys {
            match self.sc_send(title, content, key).await {
                Ok(response) => {
                    // Parse response
                    let data: Value = serde_json::from_str(&response)
                        .map_err(|e| anyhow!("Failed to parse response: {}", e))?;
                        
                    // Check if successful
                    let code = data["code"].as_i64().unwrap_or(-1);
                    if code != 0 {
                        let message = data["message"].as_str().unwrap_or("Unknown error");
                        errors.push(format!("Failed to send notification to key {}: {}", key, message));
                    } else {
                        debug!("Notification sent successfully to key: {}", key);
                    }
                },
                Err(e) => {
                    errors.push(format!("Failed to send notification to key {}: {}", key, e));
                }
            }
        }
        
        if !errors.is_empty() {
            return Err(anyhow!("Some notifications failed: {}", errors.join("; ")));
        }
        
        Ok(())
    }
} 