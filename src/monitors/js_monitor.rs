use anyhow::{Result, anyhow};
use log::{debug, info};
use headless_chrome::Browser;
use serde_json::Value;

use crate::monitors::{Monitor, Change};

/// JavaScript数据监控器
pub struct JsMonitor {
    /// 目标URL
    url: String,
    /// JavaScript选择器
    selector: String,
    /// 上次检测到的值
    last_value: Option<String>,
    /// 检查间隔（秒）
    interval_secs: u64,
}

impl JsMonitor {
    /// 创建新的JavaScript监控器
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
        debug!("JS监控检查: {}", self.url);
        
        // 创建浏览器实例
        let browser = Browser::default()
            .map_err(|e| anyhow!("创建浏览器实例失败: {}", e))?;
        
        // 创建新标签页
        let tab = browser.wait_for_initial_tab()
            .map_err(|e| anyhow!("获取初始标签页失败: {}", e))?;
        
        // 导航到目标URL
        tab.navigate_to(&self.url)
            .map_err(|e| anyhow!("导航到目标URL失败: {}", e))?;
        
        // 等待页面加载完成
        tab.wait_until_navigated()
            .map_err(|e| anyhow!("等待页面加载失败: {}", e))?;
        
        // 执行JavaScript获取数据
        let eval_result = tab.evaluate(&self.selector, false)
            .map_err(|e| anyhow!("执行JavaScript失败: {}", e))?;
            
        // 获取值并转换为字符串
        let value = match eval_result.value {
            Some(Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => return Err(anyhow!("无法获取JavaScript执行结果")),
        };
        
        debug!("获取到JavaScript值: {}", value);
        
        // 检查是否有变化
        if let Some(last_value) = &self.last_value {
            if *last_value != value {
                // 发现变化
                info!("JS数据变化 ({}): {} -> {}", self.url, last_value, value);
                
                // 更新上次值
                let old_value = last_value.clone();
                self.last_value = Some(value.clone());
                
                return Ok(Some(Change {
                    message: format!("JavaScript数据变化: {}", self.url),
                    details: format!("选择器: {}\n原值: {}\n新值: {}", self.selector, old_value, value),
                }));
            } else {
                // 无变化
                debug!("无变化");
                return Ok(None);
            }
        } else {
            // 首次检查，记录初始值
            debug!("首次检查，记录初始值: {}", value);
            self.last_value = Some(value);
            return Ok(None);
        }
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
} 