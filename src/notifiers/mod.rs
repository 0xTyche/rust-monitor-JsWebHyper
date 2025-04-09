pub mod server_chan;

use anyhow::Result;

/// Notifier trait, all types of notification services need to implement this trait
pub trait Notifier: Send + Sync {
    /// Send notification
    async fn send(&self, title: &str, content: &str) -> Result<()>;
} 