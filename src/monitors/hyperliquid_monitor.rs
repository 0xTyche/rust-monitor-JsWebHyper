use anyhow::{Result, anyhow};
use log::debug;
use std::str::FromStr;
use ethers::types::H160;
use serde_json::Value;

use crate::monitors::{Monitor, Change};

/// Hyperliquid user transaction monitor, used to monitor user transaction activities
pub struct HyperliquidMonitor {
    /// Wallet address to monitor
    address: String,
    /// Monitoring interval (seconds)
    interval_secs: u64,
    /// Whether to monitor spot transactions
    monitor_spot: bool,
    /// Whether to monitor contract transactions
    monitor_contract: bool,
    /// Last detected spot transaction ID
    last_spot_trade_id: Option<String>,
    /// Last detected contract transaction ID
    last_contract_trade_id: Option<String>,
    /// HTTP client
    client: reqwest::Client,
}

impl HyperliquidMonitor {
    /// Create a new Hyperliquid user transaction monitor
    pub fn new(address: &str, interval_secs: u64, monitor_spot: bool, monitor_contract: bool) -> Self {
        Self {
            address: address.to_string(),
            interval_secs,
            monitor_spot,
            monitor_contract,
            last_spot_trade_id: None,
            last_contract_trade_id: None,
            client: reqwest::Client::new(),
        }
    }
    
    /// Convert address string to H160 type
    fn parse_address(&self) -> Result<H160> {
        H160::from_str(&self.address)
            .map_err(|e| anyhow!("Parsing address failed: {}", e))
    }
    
    /// Get user spot transaction history
    async fn get_spot_trades(&self) -> Result<Value> {
        debug!("Getting user spot transaction history: {}", self.address);
        
        // Build API request URL
        let url = format!(
            "https://api.hyperliquid.xyz/info/spotUserFills?user={}",
            self.address
        );
        
        // Send request
        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("API request failed: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API request failed, status code: {}", status));
        }
        
        // Parse response
        let data: Value = response.json()
            .await
            .map_err(|e| anyhow!("Parsing response failed: {}", e))?;
            
        Ok(data)
    }
    
    /// Get user contract transaction history
    async fn get_contract_trades(&self) -> Result<Value> {
        debug!("Getting user contract transaction history: {}", self.address);
        
        // Build API request URL
        let url = format!(
            "https://api.hyperliquid.xyz/info/userFills?user={}",
            self.address
        );
        
        // Send request
        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("API request failed: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API request failed, status code: {}", status));
        }
        
        // Parse response
        let data: Value = response.json()
            .await
            .map_err(|e| anyhow!("Parsing response failed: {}", e))?;
            
        Ok(data)
    }
    
    /// Check user spot transaction changes
    async fn check_spot_trades(&mut self) -> Result<Option<Change>> {
        if !self.monitor_spot {
            return Ok(None);
        }
        
        let trades = self.get_spot_trades().await?;
        
        // Check if there are transaction records
        let trades_array = trades.as_array()
            .ok_or_else(|| anyhow!("API returned data format is incorrect"))?;
            
        if trades_array.is_empty() {
            debug!("No spot transaction records found");
            return Ok(None);
        }
        
        // Get latest transaction record
        let latest_trade = &trades_array[0];
        
        // Extract transaction ID
        let trade_id = latest_trade["tid"].as_str()
            .ok_or_else(|| anyhow!("Transaction ID format is incorrect"))?
            .to_string();
            
        // Check if there are new transactions
        if let Some(last_id) = &self.last_spot_trade_id {
            if last_id != &trade_id {
                // Extract transaction details
                let asset = latest_trade["asset"].as_str().unwrap_or("Unknown");
                let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "Buy" } else { "Sell" };
                let price = latest_trade["px"].as_str().unwrap_or("0");
                let size = latest_trade["sz"].as_str().unwrap_or("0");
                let time = latest_trade["time"].as_u64().unwrap_or(0);
                
                // Build change notification
                let change = Change {
                    message: format!("Detected new spot transaction: {} {}", asset, side),
                    details: format!(
                        "User: {}\nAsset: {}\nDirection: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}",
                        self.address, asset, side, price, size, time, trade_id
                    ),
                };
                
                // Update last transaction ID
                self.last_spot_trade_id = Some(trade_id);
                
                return Ok(Some(change));
            }
        } else {
            // First check, send initial notification
            debug!("First time getting spot transaction records");
            
            // Extract transaction details
            let asset = latest_trade["asset"].as_str().unwrap_or("Unknown");
            let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "Buy" } else { "Sell" };
            let price = latest_trade["px"].as_str().unwrap_or("0");
            let size = latest_trade["sz"].as_str().unwrap_or("0");
            let time = latest_trade["time"].as_u64().unwrap_or(0);
            
            // Build initial notification
            let change = Change {
                message: format!("Initial spot transaction data for {}", self.address),
                details: format!(
                    "User: {}\nLatest Transaction:\nAsset: {}\nDirection: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}",
                    self.address, asset, side, price, size, time, trade_id
                ),
            };
            
            // Update last transaction ID
            self.last_spot_trade_id = Some(trade_id);
            
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    /// Check user contract transaction changes
    async fn check_contract_trades(&mut self) -> Result<Option<Change>> {
        if !self.monitor_contract {
            return Ok(None);
        }
        
        let trades = self.get_contract_trades().await?;
        
        // Check if there are transaction records
        let trades_array = trades.as_array()
            .ok_or_else(|| anyhow!("API returned data format is incorrect"))?;
            
        if trades_array.is_empty() {
            debug!("No contract transaction records found");
            return Ok(None);
        }
        
        // Get latest transaction record
        let latest_trade = &trades_array[0];
        
        // Extract transaction ID
        let trade_id = latest_trade["tid"].as_str()
            .ok_or_else(|| anyhow!("Transaction ID format is incorrect"))?
            .to_string();
            
        // Check if there are new transactions
        if let Some(last_id) = &self.last_contract_trade_id {
            if last_id != &trade_id {
                // Extract transaction details
                let asset = latest_trade["coin"].as_str().unwrap_or("Unknown");
                let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "Buy" } else { "Sell" };
                let price = latest_trade["px"].as_str().unwrap_or("0");
                let size = latest_trade["sz"].as_str().unwrap_or("0");
                let time = latest_trade["time"].as_u64().unwrap_or(0);
                
                // Build change notification
                let change = Change {
                    message: format!("Detected new contract transaction: {} {}", asset, side),
                    details: format!(
                        "User: {}\nAsset: {}\nDirection: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}",
                        self.address, asset, 
                        if side == "Buy" { "Buy" } else { "Sell" }, 
                        price, size, time, trade_id
                    ),
                };
                
                // Update last transaction ID
                self.last_contract_trade_id = Some(trade_id);
                
                return Ok(Some(change));
            }
        } else {
            // First check, send initial notification
            debug!("First time getting contract transaction records");
            
            // Extract transaction details
            let asset = latest_trade["coin"].as_str().unwrap_or("Unknown");
            let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "Buy" } else { "Sell" };
            let price = latest_trade["px"].as_str().unwrap_or("0");
            let size = latest_trade["sz"].as_str().unwrap_or("0");
            let time = latest_trade["time"].as_u64().unwrap_or(0);
            
            // Build initial notification
            let change = Change {
                message: format!("Initial contract transaction data for {}", self.address),
                details: format!(
                    "User: {}\nLatest Transaction:\nAsset: {}\nDirection: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}",
                    self.address, asset, 
                    if side == "Buy" { "Buy" } else { "Sell" }, 
                    price, size, time, trade_id
                ),
            };
            
            // Update last transaction ID
            self.last_contract_trade_id = Some(trade_id);
            
            return Ok(Some(change));
        }
        
        Ok(None)
    }
}

#[async_trait::async_trait]
impl Monitor for HyperliquidMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        // Check spot transactions
        if let Some(change) = self.check_spot_trades().await? {
            return Ok(Some(change));
        }
        
        // Check contract transactions
        if let Some(change) = self.check_contract_trades().await? {
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
    
    fn get_name(&self) -> String {
        format!("Hyperliquid account monitor for {}", self.address)
    }
} 