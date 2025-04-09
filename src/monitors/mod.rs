pub mod js_monitor;
pub mod static_monitor;
pub mod hyperliquid_monitor;

use anyhow::Result;
use std::fmt::Display;

/// 监控到的变化信息
#[derive(Clone)]
pub struct Change {
    /// 变化摘要信息
    pub message: String,
    /// 变化详细信息
    pub details: String,
}

/// 监控器特性，所有类型的监控器都需要实现该特性
pub trait Monitor: Send + Sync {
    /// 执行一次检查，返回变化信息或错误
    async fn check(&mut self) -> Result<Option<Change>>;
    
    /// 获取监控间隔（秒）
    fn interval(&self) -> u64;
}

impl Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
} 