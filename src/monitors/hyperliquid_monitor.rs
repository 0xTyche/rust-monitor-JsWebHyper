use anyhow::{Result, anyhow};
use log::{debug, info};
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
    /// Last positions hash value to detect position changes
    last_positions_hash: Option<String>,
    /// HTTP client
    client: reqwest::Client,
    /// User-provided notes/remarks
    notes: String,
}

/// 持仓信息结构
#[derive(Debug, Clone)]
struct PositionInfo {
    /// 资产名称
    asset: String,
    /// 杠杆倍数
    leverage: f64,
    /// 持仓类型（多/空）
    position_type: String,
    /// 入场价格
    entry_price: f64,
    /// 标记价格
    mark_price: f64,
    /// 持仓大小
    size: f64,
    /// 仓位价值
    position_value: f64,
    /// 收益百分比
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
            notes: address.to_string(), // 默认使用地址作为备注
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
        
        // Build API request URL for user info
        let url = format!(
            "https://api.hyperliquid.xyz/info/positions?user={}",
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
            
        // Parse positions from response
        let positions = parse_positions(&data)?;
        
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
                
                // 格式化交易时间
                let formatted_time = format_timestamp(time);
                
                // 创建变化描述
                let change_description = format!(
                    "新的{}{}: 资产:{}，价格:{}，数量:{}，时间:{}",
                    asset, side, asset, price, size, formatted_time
                );
                
                // Build change notification
                let change = Change {
                    message: format!("Detected new spot transaction: {} {}", asset, side),
                    details: format!(
                        "变化的内容：\n{}\n\n当前的交易：\n用户: {}\n资产: {}\n方向: {}\n价格: {}\n数量: {}\n时间: {}\n交易ID: {}\n\n之前的交易ID：\n{}",
                        change_description,
                        self.address, asset, side, price, size, formatted_time, trade_id,
                        last_id
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
            
            // 格式化交易时间
            let formatted_time = format_timestamp(time);
            
            // Build initial notification
            let change = Change {
                message: format!("Initial spot transaction data for {}", self.address),
                details: format!(
                    "首次监控数据：\n用户: {}\n最新交易:\n资产: {}\n方向: {}\n价格: {}\n数量: {}\n时间: {}\n交易ID: {}",
                    self.address, asset, side, price, size, formatted_time, trade_id
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
            return Ok(None);
        }
        
        // Get current positions
        let positions = self.get_contract_positions().await?;
        
        // Calculate hash of current positions (空持仓也需要计算hash值)
        let positions_hash = self.calculate_positions_hash(&positions);
        
        // No positions
        if positions.is_empty() {
            debug!("No contract positions found for user: {}", self.address);
            
            // 首次检查或持仓变更为空都需要发送通知
            if self.last_positions_hash.is_some() {
                // 之前有持仓现在没有了 - 平仓通知
                let change = Change {
                    message: format!("close：{}", self.notes),
                    details: format!(
                        "用户: {}\n\n当前没有活跃持仓\n\n查看更多信息：@https://hyperdash.info/trader/{}",
                        self.address, self.address
                    ),
                };
                
                // Reset hash
                self.last_positions_hash = None;
                
                return Ok(Some(change));
            } else {
                // 首次检查且无持仓 - 发送初始空持仓通知
                let change = Change {
                    message: format!("start：{}", self.notes),
                    details: format!(
                        "开始监控用户: {}\n\n当前没有活跃持仓\n\n查看更多信息：@https://hyperdash.info/trader/{}",
                        self.address, self.address
                    ),
                };
                
                // 记录空持仓的hash
                self.last_positions_hash = Some(positions_hash);
                
                return Ok(Some(change));
            }
        }
        
        // Check if positions have changed
        if let Some(last_hash) = &self.last_positions_hash {
            if *last_hash != positions_hash {
                // Positions have changed
                debug!("Contract positions changed for user: {}", self.address);
                
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
                        "资产: {}\n杠杆: {:.0}x\n类型: {}\n入场价格: {:.2}\n标记价格: {:.2}\n仓位大小: {:.4}\n仓位价值: ${:.2}\nPNL: {:.2}%\n\n",
                        pos.asset, pos.leverage, pos.position_type, 
                        pos.entry_price, pos.mark_price, pos.size, 
                        pos.position_value, pos.pnl_percentage
                    ));
                }
                
                // Build change notification with notes as the remark
                let change = Change {
                    message: format!("{} {}", self.notes, title_parts.join(" | ")),
                    details: format!(
                        "用户持仓变更:\n\n{}\n查看更多信息：@https://hyperdash.info/trader/{}",
                        position_details.trim(), self.address
                    ),
                };
                
                // Update last positions hash
                self.last_positions_hash = Some(positions_hash);
                
                return Ok(Some(change));
            }
        } else {
            // First check, send initial notification with positions
            debug!("First time getting contract positions for user: {}", self.address);
            
            // Create detailed position message
            let mut position_details = String::new();
            let mut position_info_for_title = String::new();
            
            // Format all positions for details
            for pos in &positions {
                // 为标题准备第一个持仓的信息
                if position_info_for_title.is_empty() {
                    position_info_for_title = format!("Asset:{} Lever:{:.0}x Type:{} Entry price:{:.2}",
                        pos.asset, pos.leverage, pos.position_type, pos.entry_price);
                }
                
                position_details.push_str(&format!(
                    "资产: {}\n杠杆: {:.0}x\n类型: {}\n入场价格: {:.2}\n标记价格: {:.2}\n仓位大小: {:.4}\n仓位价值: ${:.2}\nPNL: {:.2}%\n\n",
                    pos.asset, pos.leverage, pos.position_type, 
                    pos.entry_price, pos.mark_price, pos.size, 
                    pos.position_value, pos.pnl_percentage
                ));
            }
            
            // Build change notification - 首次通知使用"start：备注"格式
            let change = Change {
                message: format!("start：{}", self.notes),
                details: format!(
                    "用户当前持仓：\n\n{}\n查看更多信息：@https://hyperdash.info/trader/{}",
                    position_details.trim(), self.address
                ),
            };
            
            // Update last positions hash
            self.last_positions_hash = Some(positions_hash);
            
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    /// Check user contract transaction changes (已保留旧代码作为备份)
    async fn check_contract_trades(&mut self) -> Result<Option<Change>> {
        // 优先检查持仓变化
        if let Some(change) = self.check_contract_positions().await? {
            return Ok(Some(change));
        }
        
        if !self.monitor_contract {
            return Ok(None);
        }
        
        // 原有代码 - 检查交易历史变化
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
                
                // 已由持仓监控替代，仅更新ID但不发送通知
                return Ok(None);
            }
        } else {
            // First check, just record the ID
            self.last_contract_trade_id = Some(trade_id);
            // 已由持仓监控替代，仅记录ID但不发送通知
            return Ok(None);
        }
        
        Ok(None)
    }
}

/// 辅助函数：解析持仓数据
fn parse_positions(data: &Value) -> Result<Vec<PositionInfo>> {
    let mut positions = Vec::new();
    
    // 尝试解析JSON响应中的持仓数据
    if let Some(positions_array) = data.as_array() {
        for pos in positions_array {
            if let (Some(coin), Some(position)) = (pos.get("coin"), pos.get("position")) {
                
                // 解析基本数据
                let asset = coin.as_str().unwrap_or("Unknown").to_string();
                
                // 解析持仓方向和大小
                let szi = position["szi"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let position_type = if szi > 0.0 { "long".to_string() } else { "short".to_string() };
                let size = szi.abs();
                
                // 解析入场价格和标记价格
                let entry_price = position["entryPx"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let mark_price = pos["markPx"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                
                // 解析杠杆
                let leverage = pos["leverage"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                
                // 计算仓位价值和收益率
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
            }
        }
    }
    
    Ok(positions)
}

/// 辅助函数：格式化时间戳
fn format_timestamp(timestamp: u64) -> String {
    use chrono::{TimeZone, Local};
    match Local.timestamp_opt(timestamp as i64 / 1000, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => format!("{}(无效时间戳)", timestamp),
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
        format!("Hyperliquid account monitor for {}", self.address)
    }
} 