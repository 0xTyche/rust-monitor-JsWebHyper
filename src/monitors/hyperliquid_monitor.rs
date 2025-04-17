use anyhow::{Result, anyhow};
use log::{debug, info};
use std::str::FromStr;
use ethers::types::H160;
use serde_json::{Value, json};
use reqwest::header;

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
    /// Last positions hash value to detect position changes
    last_positions_hash: Option<String>,
    /// HTTP client
    client: reqwest::Client,
    /// User-provided notes/remarks
    notes: String,
}

/// Position information structure
#[derive(Debug, Clone)]
struct PositionInfo {
    /// Asset name
    asset: String,
    /// Leverage multiplier
    leverage: f64,
    /// Position type (long/short)
    position_type: String,
    /// Entry price
    entry_price: f64,
    /// Mark price
    mark_price: f64,
    /// Position size
    size: f64,
    /// Position value
    position_value: f64,
    /// Profit/Loss percentage
    pnl_percentage: f64,
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
            last_positions_hash: None,
            client: reqwest::Client::new(),
            notes: address.to_string(), // Default to using address as the note
        }
    }
    
    /// Create a new Hyperliquid user transaction monitor with notes
    pub fn new_with_notes(address: &str, interval_secs: u64, monitor_spot: bool, monitor_contract: bool, notes: &str) -> Self {
        let mut monitor = Self::new(address, interval_secs, monitor_spot, monitor_contract);
        if !notes.trim().is_empty() {
            monitor.notes = notes.to_string();
        }
        monitor
    }
    
    /// Set notes/remarks
    pub fn set_notes(&mut self, notes: &str) {
        if !notes.trim().is_empty() {
            self.notes = notes.to_string();
        }
    }
    
    /// Convert address string to H160 type
    fn parse_address(&self) -> Result<H160> {
        H160::from_str(&self.address)
            .map_err(|e| anyhow!("Parsing address failed: {}", e))
    }
    
    /// Get user contract positions
    async fn get_contract_positions(&self) -> Result<Vec<PositionInfo>> {
        debug!("Getting user contract positions: {}", self.address);
        
        // API endpoint
        let url = "https://api.hyperliquid.xyz/info";
        
        // Create request body - Use the proper request type for positions
        let data = json!({
            "type": "clearinghouseState",
            "user": self.address
        });
        
        // Send POST request
        let response = self.client.post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&data)
            .send()
            .await
            .map_err(|e| anyhow!("API request failed: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API request failed, status code: {}", status));
        }
        
        // Parse response and add debug logging
        let json: Value = response.json()
            .await
            .map_err(|e| anyhow!("Parsing response failed: {}", e))?;
        
        debug!("Position API response: {}", json.to_string());
        
        // Extract positions from assetPositions field
        let positions = if let Some(positions) = json.get("assetPositions") {
            parse_positions(positions)?
        } else {
            debug!("No assetPositions field found in response");
            vec![] // Return empty Vec if no positions found
        };
        
        debug!("Parsed {} positions", positions.len());
        if !positions.is_empty() {
            debug!("Position details: {:?}", positions);
        }
        
        Ok(positions)
    }
    
    /// Calculate hash of positions to detect changes
    fn calculate_positions_hash(&self, positions: &[PositionInfo]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        
        for pos in positions {
            pos.asset.hash(&mut hasher);
            format!("{:.2}", pos.size).hash(&mut hasher);
            format!("{:.2}", pos.entry_price).hash(&mut hasher);
            pos.position_type.hash(&mut hasher);
        }
        
        format!("{:x}", hasher.finish())
    }
    
    /// Get user spot transaction history
    async fn get_spot_trades(&self) -> Result<Value> {
        debug!("Getting user spot transaction history: {}", self.address);
        
        // API endpoint
        let url = "https://api.hyperliquid.xyz/info";
        
        // Create request body
        let data = json!({
            "type": "userFills",
            "user": self.address
        });
        
        // Send POST request
        let response = self.client.post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&data)
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
        
        // API endpoint
        let url = "https://api.hyperliquid.xyz/info";
        
        // Create request body
        let data = json!({
            "type": "userFills",
            "user": self.address
        });
        
        // Send POST request
        let response = self.client.post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&data)
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
                
                // Format transaction time
                let formatted_time = format_timestamp(time);
                
                // Create change description
                let change_description = format!(
                    "New {} {}: Asset:{}, Price:{}, Size:{}, Time:{}",
                    asset, side, asset, price, size, formatted_time
                );
                
                // Build change notification with notes
                let change = Change {
                    message: format!("{} - {}", self.notes, change_description),
                    details: format!(
                        "Changed content:\nUser: {}\nAsset: {}\nSide: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}\n\nPrevious transaction ID: {}\n\nNotes: {}",
                        self.address, asset, side, price, size, formatted_time, trade_id,
                        last_id, self.notes
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
            
            // Format transaction time
            let formatted_time = format_timestamp(time);
            
            // Build initial notification with notes
            let change = Change {
                message: format!("Started monitoring: {}", self.notes),
                details: format!(
                    "Initial monitoring data:\nUser: {}\nLatest transaction:\nAsset: {}\nSide: {}\nPrice: {}\nSize: {}\nTime: {}\nTransaction ID: {}\n\nNotes: {}",
                    self.address, asset, side, price, size, formatted_time, trade_id, self.notes
                ),
            };
            
            // Update last transaction ID
            self.last_spot_trade_id = Some(trade_id);
            
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    /// Check user contract positions changes
    async fn check_contract_positions(&mut self) -> Result<Option<Change>> {
        if !self.monitor_contract {
            debug!("Contract monitoring is disabled");
            return Ok(None);
        }
        
        // Get current positions
        let positions = self.get_contract_positions().await?;
        debug!("Retrieved {} positions", positions.len());
        
        // Calculate hash of current positions
        let positions_hash = self.calculate_positions_hash(&positions);
        debug!("Current positions hash: {}, last hash: {:?}", positions_hash, self.last_positions_hash);
        
        // Check for first run
        if self.last_positions_hash.is_none() {
            debug!("First check for user: {}, positions count: {}", self.address, positions.len());
            
            // First check with positions
            let change = if positions.is_empty() {
                debug!("Initial check with no positions");
                Change {
                    message: format!("Started monitoring: {}", self.notes),
                    details: format!(
                        "Started monitoring user: {}\n\nNo active positions currently\n\nView more information: @https://hyperdash.info/trader/{}\n\nNotes: {}",
                        self.address, self.address, self.notes
                    ),
                }
            } else {
                debug!("Initial check with {} positions", positions.len());
                
                // Create detailed position message
                let mut position_details = String::new();
                let mut position_info_for_title = String::new();
                
                // Format all positions for details
                for pos in &positions {
                    // Prepare first position info for title
                    if position_info_for_title.is_empty() {
                        position_info_for_title = format!("Asset:{} Lever:{:.0}x Type:{} Entry price:{:.2}",
                            pos.asset, pos.leverage, pos.position_type, pos.entry_price);
                    }
                    
                    position_details.push_str(&format!(
                        "Asset: {}\nLeverage: {:.0}x\nType: {}\nEntry price: {:.2}\nMark price: {:.2}\nPosition size: {:.4}\nPosition value: ${:.2}\nPNL: {:.2}%\n\n",
                        pos.asset, pos.leverage, pos.position_type, 
                        pos.entry_price, pos.mark_price, pos.size, 
                        pos.position_value, pos.pnl_percentage
                    ));
                }
                
                // Build change notification with notes
                Change {
                    message: format!("Started monitoring: {}", self.notes),
                    details: format!(
                        "User's current positions:\n\n{}\nView more information: @https://hyperdash.info/trader/{}\n\nNotes: {}",
                        position_details.trim(), self.address, self.notes
                    ),
                }
            };
            
            // Update last positions hash and return the change
            let hash_clone = positions_hash.clone();
            self.last_positions_hash = Some(positions_hash);
            debug!("Updated last positions hash on first check: {}", hash_clone);
            debug!("Returning change: {}", change.message);
            return Ok(Some(change));
        }
        
        // For subsequent checks, compare hash with previous
        if let Some(last_hash) = &self.last_positions_hash {
            debug!("Comparing position hashes - current: {}, last: {}", positions_hash, last_hash);
            
            if *last_hash != positions_hash {
                debug!("Position hash changed: {} -> {}", last_hash, positions_hash);
                
                // Positions have changed
                let change = if positions.is_empty() {
                    debug!("Positions changed to empty");
                    Change {
                        message: format!("No active positions - {}", self.notes),
                        details: format!(
                            "User: {}\n\nNo active positions currently\n\nView more information: @https://hyperdash.info/trader/{}\n\nNotes: {}",
                            self.address, self.address, self.notes
                        ),
                    }
                } else {
                    debug!("Positions changed, now has {} positions", positions.len());
                    
                    // Create detailed position message
                    let mut position_details = String::new();
                    let mut title_parts = Vec::new();
                    
                    // Format the first position for the title
                    if !positions.is_empty() {
                        let first_pos = &positions[0];
                        title_parts.push(format!("Asset:{} Lever:{:.0}x Type:{} Entry price:{:.2}",
                            first_pos.asset, first_pos.leverage, 
                            first_pos.position_type, first_pos.entry_price));
                    }
                    
                    // Format all positions for details
                    for pos in &positions {
                        position_details.push_str(&format!(
                            "Asset: {}\nLeverage: {:.0}x\nType: {}\nEntry price: {:.2}\nMark price: {:.2}\nPosition size: {:.4}\nPosition value: ${:.2}\nPNL: {:.2}%\n\n",
                            pos.asset, pos.leverage, pos.position_type, 
                            pos.entry_price, pos.mark_price, pos.size, 
                            pos.position_value, pos.pnl_percentage
                        ));
                    }
                    
                    // Build change notification with notes
                    Change {
                        message: format!("{} - {}", self.notes, title_parts.join(" | ")),
                        details: format!(
                            "User position changes:\n\n{}\nView more information: @https://hyperdash.info/trader/{}\n\nNotes: {}",
                            position_details.trim(), self.address, self.notes
                        ),
                    }
                };
                
                // Update last positions hash
                let hash_clone = positions_hash.clone();
                self.last_positions_hash = Some(positions_hash);
                debug!("Updated positions hash after change: {}", hash_clone);
                debug!("Returning change: {}", change.message);
                return Ok(Some(change));
            } else {
                debug!("No position changes detected, hash remained: {}", positions_hash);
            }
        }
        
        Ok(None)
    }
    
    /// Check user contract transaction changes (old code kept as backup)
    async fn check_contract_trades(&mut self) -> Result<Option<Change>> {
        // Check position changes first
        if let Some(change) = self.check_contract_positions().await? {
            return Ok(Some(change));
        }
        
        if !self.monitor_contract {
            return Ok(None);
        }
        
        // Original code - Check transaction history changes
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
                // Update last transaction ID
                self.last_contract_trade_id = Some(trade_id);
                
                // Replaced by position monitoring, only update ID without sending notification
                return Ok(None);
            }
        } else {
            // First check, just record the ID
            self.last_contract_trade_id = Some(trade_id);
            // Replaced by position monitoring, only record ID without sending notification
            return Ok(None);
        }
        
        Ok(None)
    }
}

/// Helper function: Parse position data
fn parse_positions(data: &Value) -> Result<Vec<PositionInfo>> {
    let mut positions = Vec::new();
    
    // Try to parse position data from JSON response
    if let Some(positions_array) = data.as_array() {
        debug!("Found {} position entries to parse", positions_array.len());
        
        for pos in positions_array {
            debug!("Processing position: {}", pos);
            
            // Updated position parsing logic to match API response format
            let asset = pos.get("coin").and_then(|c| c.as_str()).unwrap_or("Unknown").to_string();
            
            if let Some(position) = pos.get("position") {
                // Parse position direction and size
                let szi = position.get("szi").and_then(|s| s.as_str()).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                if szi == 0.0 {
                    debug!("Skipping position with zero size for {}", asset);
                    continue; // Skip positions with zero size
                }
                
                let position_type = if szi > 0.0 { "long".to_string() } else { "short".to_string() };
                let size = szi.abs();
                
                // Parse entry price and mark price
                let entry_price = position.get("entryPx").and_then(|p| p.as_str()).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let mark_price = pos.get("markPx").and_then(|p| p.as_str()).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                
                // Parse leverage
                let leverage = pos.get("leverage").and_then(|l| l.as_str()).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                
                // Calculate position value and profit/loss percentage
                let position_value = size * mark_price;
                let entry_value = size * entry_price;
                let pnl = if position_type == "long" {
                    position_value - entry_value
                } else {
                    entry_value - position_value
                };
                let pnl_percentage = if entry_value > 0.0 {
                    (pnl / entry_value) * 100.0
                } else {
                    0.0
                };
                
                debug!("Successfully parsed position: {} {} size={} entry={} mark={}", 
                       asset, position_type, size, entry_price, mark_price);
                
                positions.push(PositionInfo {
                    asset,
                    leverage,
                    position_type,
                    entry_price,
                    mark_price,
                    size,
                    position_value,
                    pnl_percentage,
                });
            } else {
                debug!("Position field not found for {}", asset);
            }
        }
    } else {
        debug!("Position data is not an array");
    }
    
    debug!("Returning {} parsed positions", positions.len());
    Ok(positions)
}

/// Helper function: Format timestamp
fn format_timestamp(timestamp: u64) -> String {
    use chrono::{TimeZone, Local};
    match Local.timestamp_opt(timestamp as i64 / 1000, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => format!("{}(Invalid timestamp)", timestamp),
    }
}

#[async_trait::async_trait]
impl Monitor for HyperliquidMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        // Check spot transactions
        if let Some(change) = self.check_spot_trades().await? {
            return Ok(Some(change));
        }
        
        // Check contract positions and transactions
        if let Some(change) = self.check_contract_positions().await? {
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
    
    fn get_name(&self) -> String {
        format!("Hyperliquid monitor for {}", self.address)
    }

    fn get_notes(&self) -> String {
        self.notes.clone()
    }
} 