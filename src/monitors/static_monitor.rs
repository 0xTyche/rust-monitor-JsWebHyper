use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use scraper::{Html, Selector};
use std::time::Duration;

use crate::monitors::{Monitor, Change};

/// 静态网页监控器，用于监控网页内容变化
pub struct StaticMonitor {
    /// 要监控的网页URL
    url: String,
    /// HTML选择器
    selector: String,
    /// 监控间隔（秒）
    interval_secs: u64,
    /// 上次检测到的内容
    last_content: Option<String>,
    /// HTTP客户端
    client: Client,
}

impl StaticMonitor {
    /// 创建一个新的静态网页监控器
    pub fn new(url: &str, selector: &str, interval_secs: u64) -> Self {
        // 创建HTTP客户端，设置超时
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
    
    /// 获取网页中指定元素的内容
    async fn get_content(&self) -> Result<String> {
        debug!("获取网页内容: {} - {}", self.url, self.selector);
        
        // 发送HTTP请求获取网页内容
        let response = self.client.get(&self.url)
            .send()
            .await
            .map_err(|e| anyhow!("获取网页内容失败: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP请求失败，状态码: {}", status));
        }
        
        let html = response.text()
            .await
            .map_err(|e| anyhow!("读取响应内容失败: {}", e))?;
            
        // 解析HTML
        let document = Html::parse_document(&html);
        
        // 解析选择器
        let selector = Selector::parse(&self.selector)
            .map_err(|e| anyhow!("解析选择器失败: {}", e))?;
            
        // 获取匹配元素的内容
        let content = document.select(&selector)
            .map(|element| element.inner_html())
            .collect::<Vec<String>>()
            .join("\n");
            
        if content.is_empty() {
            return Err(anyhow!("未找到匹配的元素"));
        }
        
        debug!("获取到内容: {} 字节", content.len());
        
        Ok(content)
    }
}

impl Monitor for StaticMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        match self.get_content().await {
            Ok(current_content) => {
                // 检查内容是否发生变化
                if let Some(last_content) = &self.last_content {
                    if *last_content != current_content {
                        // 内容发生变化
                        let change = Change {
                            message: format!("网页内容发生变化: {}", self.url),
                            details: format!(
                                "选择器: {}\n内容长度: {} -> {} 字节", 
                                self.selector, 
                                last_content.len(), 
                                current_content.len()
                            ),
                        };
                        
                        // 更新上次内容
                        self.last_content = Some(current_content);
                        
                        return Ok(Some(change));
                    }
                } else {
                    // 首次检查，不触发变化通知
                    debug!("首次获取内容: {} 字节", current_content.len());
                    self.last_content = Some(current_content);
                }
                
                Ok(None)
            }
            Err(e) => {
                error!("获取网页内容失败: {}", e);
                Err(anyhow!("获取网页内容失败: {}", e))
            }
        }
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
} 