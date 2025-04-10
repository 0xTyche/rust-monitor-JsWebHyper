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
    
    /// 生成更易读的变化描述
    fn generate_change_description(&self, old_content: &str, new_content: &str) -> String {
        // 获取字符串长度的变化
        let old_len = old_content.len();
        let new_len = new_content.len();
        
        let mut changes = String::new();
        
        // 检查内容长度变化
        if new_len > old_len {
            changes.push_str(&format!("内容增加: {} -> {} 字节 (增加 {} 字节)\n", 
                old_len, new_len, new_len - old_len));
        } else if new_len < old_len {
            changes.push_str(&format!("内容减少: {} -> {} 字节 (减少 {} 字节)\n", 
                old_len, new_len, old_len - new_len));
        } else {
            changes.push_str("内容长度相同，但内容已变化\n");
        }
        
        // 尝试检测一些常见的HTML变化
        if old_content.contains("<title>") && new_content.contains("<title>") {
            // 提取标题
            let old_title = extract_between(old_content, "<title>", "</title>").unwrap_or("未找到");
            let new_title = extract_between(new_content, "<title>", "</title>").unwrap_or("未找到");
            
            if old_title != new_title {
                changes.push_str(&format!("标题变化: '{}' -> '{}'\n", old_title, new_title));
            }
        }
        
        changes
    }
}

/// 辅助函数：提取两个标记之间的内容
fn extract_between<'a>(content: &'a str, start_marker: &str, end_marker: &str) -> Option<&'a str> {
    if let Some(start_idx) = content.find(start_marker) {
        let content_after_start = &content[start_idx + start_marker.len()..];
        if let Some(end_idx) = content_after_start.find(end_marker) {
            return Some(&content_after_start[..end_idx]);
        }
    }
    None
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
                        // 创建更易读的变化描述
                        let change_description = self.generate_change_description(last_content, &current_content);
                        
                        let change = Change {
                            message: format!("Webpage content changed: {}", self.url),
                            details: format!(
                                "变化的内容：\n{}\n\n当前的内容长度：{} 字节\n\n之前的内容长度：{} 字节", 
                                change_description,
                                current_content.len(), 
                                last_content.len()
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
                        details: format!("初始内容长度: {} 字节", current_content.len()),
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