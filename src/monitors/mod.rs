pub mod js_monitor;
pub mod static_monitor;
pub mod hyperliquid_monitor;

use anyhow::Result;
use std::fmt::Display;

/// Change information detected by monitors
#[derive(Clone)]
pub struct Change {
    /// Change summary message
    pub message: String,
    /// Change detailed information
    pub details: String,
}

/// Monitor trait, all types of monitors need to implement this trait
pub trait Monitor: Send + Sync {
    /// Execute a check, returns change information or error
    async fn check(&mut self) -> Result<Option<Change>>;
    
    /// Get monitoring interval (seconds)
    fn interval(&self) -> u64;
}

impl Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
} 