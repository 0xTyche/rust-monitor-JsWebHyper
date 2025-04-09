pub mod server_chan;

use anyhow::Result;

/// 通知服务特性，所有类型的通知服务都需要实现该特性
pub trait Notifier: Send + Sync {
    /// 发送通知
    async fn send(&self, title: &str, content: &str) -> Result<()>;
} 