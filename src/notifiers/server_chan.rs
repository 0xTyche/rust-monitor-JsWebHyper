use anyhow::{Result, anyhow};
use log::{debug, error};
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, CONTENT_LENGTH};
use serde_json::Value;
use serde_urlencoded;
use regex::Regex;

use crate::notifiers::Notifier;

/// Server酱通知服务，用于发送通知到微信
pub struct ServerChanNotifier {
    /// Server酱的SCKEY
    key: String,
    /// HTTP客户端
    client: Client,
}

impl ServerChanNotifier {
    /// 创建一个新的Server酱通知服务
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            client: Client::new(),
        }
    }
    
    /// 使用方糖提供的sc_send方法发送通知
    async fn sc_send(&self, text: &str, desp: &str) -> Result<String> {
        let params = [("text", text), ("desp", desp)];
        let post_data = serde_urlencoded::to_string(params)
            .map_err(|e| anyhow!("编码请求参数失败: {}", e))?;
            
        // 使用正则表达式提取 key 中的数字部分
        let url = if self.key.starts_with("sctp") {
            let re = Regex::new(r"sctp(\d+)t")
                .map_err(|e| anyhow!("正则表达式解析失败: {}", e))?;
                
            if let Some(captures) = re.captures(&self.key) {
                let num = &captures[1]; // 提取正则表达式捕获的数字部分
                format!("https://{}.push.ft07.com/send/{}.send", num, self.key)
            } else {
                return Err(anyhow!("Server酱密钥格式不正确"));
            }
        } else {
            format!("https://sctapi.ftqq.com/{}.send", self.key)
        };
        
        debug!("发送通知到: {}", url);
        
        let res = self.client.post(&url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(CONTENT_LENGTH, post_data.len() as u64)
            .body(post_data)
            .send()
            .await
            .map_err(|e| anyhow!("发送通知请求失败: {}", e))?;
            
        let status = res.status();
        if !status.is_success() {
            return Err(anyhow!("发送通知请求失败，状态码: {}", status));
        }
            
        let data = res.text().await
            .map_err(|e| anyhow!("读取响应内容失败: {}", e))?;
            
        debug!("通知响应: {}", data);
        
        Ok(data)
    }
}

impl Notifier for ServerChanNotifier {
    async fn send(&self, title: &str, content: &str) -> Result<()> {
        if self.key.is_empty() {
            error!("Server酱密钥未设置，无法发送通知");
            return Err(anyhow!("Server酱密钥未设置"));
        }
        
        debug!("发送Server酱通知: {}", title);
        
        // 使用方糖提供的sc_send方法发送通知
        let response = self.sc_send(title, content).await?;
        
        // 解析响应
        let data: Value = serde_json::from_str(&response)
            .map_err(|e| anyhow!("解析响应失败: {}", e))?;
            
        // 检查是否成功
        let code = data["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            let message = data["message"].as_str().unwrap_or("未知错误");
            return Err(anyhow!("发送通知失败: {}", message));
        }
        
        debug!("通知发送成功");
        
        Ok(())
    }
} 