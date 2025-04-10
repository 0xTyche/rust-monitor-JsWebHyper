use log::{debug, info};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use jsonpath_lib as jsonpath;
use anyhow::{Result, anyhow};

use crate::monitors::{Change, Monitor};

/// Monitor JSON data returned from API
pub struct ApiMonitor {
    /// API URL
    url: String,
    /// JSONPath selector
    selector: String,
    /// Last detected value
    last_value: Option<String>,
    /// Check interval (seconds)
    interval_secs: u64,
}

impl ApiMonitor {
    /// Create a new API monitor
    pub fn new(url: String, selector: String, interval_secs: u64) -> Self {
        ApiMonitor {
            url,
            selector,
            last_value: None,
            interval_secs,
        }
    }
}

#[async_trait::async_trait]
impl Monitor for ApiMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        info!("Checking API at {}", self.url);
        
        let client = Client::new();
        let response = match client
            .get(&self.url)
            .timeout(Duration::from_secs(30))
            .send()
            .await {
                Ok(resp) => resp,
                Err(e) => {
                    debug!("Failed to fetch API: {}", e);
                    return Ok(Some(Change {
                        message: format!("API request failed: {}", e),
                        details: format!("URL: {}", self.url),
                    }));
                }
            };
            
        if !response.status().is_success() {
            debug!("API returned non-success status code: {}", response.status());
            return Ok(Some(Change {
                message: format!("API returned status code {}", response.status()),
                details: format!("URL: {}", self.url),
            }));
        }
        
        let json: Value = match response.json::<Value>().await {
            Ok(json) => {
                // 添加调试日志，输出完整的JSON响应
                debug!("Received JSON response: {}", json.to_string());
                json
            },
            Err(e) => {
                debug!("Failed to parse JSON response: {}", e);
                return Ok(Some(Change {
                    message: format!("Failed to parse JSON response: {}", e),
                    details: format!("URL: {}", self.url),
                }));
            }
        };
        
        // Extract data using JSONPath
        let selector = self.selector.trim();
        let result = match jsonpath::select(&json, selector) {
            Ok(results) if !results.is_empty() => {
                // 处理多个结果，不仅仅是第一个
                let result_str = if results.len() == 1 {
                    // 单个结果的处理方式
                    results[0].to_string().trim_matches('"').to_string()
                } else {
                    // 多个结果的处理方式 - 将所有结果合并成一个JSON数组字符串
                    let values: Vec<String> = results.iter()
                        .map(|r| r.to_string().trim_matches('"').to_string())
                        .collect();
                    format!("[{}]", values.join(", "))
                };
                
                Some(result_str)
            },
            Ok(_) => {
                debug!("JSONPath selector returned no results");
                None
            },
            Err(e) => {
                debug!("JSONPath selector error: {}", e);
                return Err(anyhow!("JSONPath selector error: {}", e));
            }
        };
        
        match &self.last_value {
            None => {
                // First check
                if let Some(new_value) = result {
                    debug!("First check, recording initial value: {}", new_value);
                    
                    // Create change object with initial value
                    let change = Change {
                        message: format!("Initial API data from {}", self.url),
                        details: format!("JSONPath: {}\nInitial value: {}\n\nNote: This may represent multiple values if your JSONPath selector matches multiple elements.", 
                            selector, new_value),
                    };
                    
                    // Set the last value
                    self.last_value = Some(new_value);
                    
                    // Return the change to send initial notification
                    Ok(Some(change))
                } else {
                    // Could not extract initial data
                    debug!("Could not extract initial data using selector: {}", self.selector);
                    self.last_value = None;
                    Ok(Some(Change {
                        message: format!("Could not extract initial data from API response"),
                        details: format!("URL: {}\nSelector: {}\n\nThe JSONPath selector did not match any data. Please check if your selector is correct.", 
                            self.url, self.selector),
                    }))
                }
            }
            Some(old_value) => {
                if let Some(new_value) = result {
                    if *old_value != new_value {
                        // Change detected
                        info!("Detected change in API data");
                        debug!("Old value: {}", old_value);
                        debug!("New value: {}", new_value);
                        
                        // Create change object with old_value (already borrowed)
                        let change = Change {
                            message: format!("API data changed at {}", self.url),
                            details: format!("JSONPath: {}\nOld value: {}\nNew value: {}\n\nNote: If your JSONPath selector matches multiple elements, this represents the combined changes.", 
                                selector, old_value, &new_value),
                        };
                        
                        // Now update the last_value after we've used old_value
                        self.last_value = Some(new_value);
                        
                        Ok(Some(change))
                    } else {
                        // No change
                        debug!("No change detected");
                        // Since we don't need the old value anymore, we can update
                        self.last_value = Some(new_value);
                        Ok(None)
                    }
                } else {
                    // Could not extract data
                    debug!("Could not extract data using selector: {}", self.selector);
                    Ok(Some(Change {
                        message: format!("Could not extract data from API response"),
                        details: format!("URL: {}\nSelector: {}\n\nThe JSONPath selector did not match any data after a previous successful match. The data structure may have changed.", 
                            self.url, self.selector),
                    }))
                }
            }
        }
    }

    fn interval(&self) -> u64 {
        self.interval_secs
    }
    
    fn get_name(&self) -> String {
        format!("API monitor for {}", self.url)
    }
} 