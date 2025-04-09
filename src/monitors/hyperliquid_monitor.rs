use anyhow::{Result, anyhow};
use log::debug;
use std::str::FromStr;
use ethers::types::H160;
use serde_json::Value;

use crate::monitors::{Monitor, Change};

/// Hyperliquid用户交易监控器，用于监控用户的交易活动
pub struct HyperliquidMonitor {
    /// 要监控的钱包地址
    address: String,
    /// 监控间隔（秒）
    interval_secs: u64,
    /// 是否监控现货交易
    monitor_spot: bool,
    /// 是否监控合约交易
    monitor_contract: bool,
    /// 上次检测到的现货交易ID
    last_spot_trade_id: Option<String>,
    /// 上次检测到的合约交易ID
    last_contract_trade_id: Option<String>,
    /// HTTP客户端
    client: reqwest::Client,
}

impl HyperliquidMonitor {
    /// 创建一个新的Hyperliquid用户交易监控器
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
    
    /// 将地址字符串转换为H160类型
    fn parse_address(&self) -> Result<H160> {
        H160::from_str(&self.address)
            .map_err(|e| anyhow!("解析地址失败: {}", e))
    }
    
    /// 获取用户的现货交易历史
    async fn get_spot_trades(&self) -> Result<Value> {
        debug!("获取用户现货交易历史: {}", self.address);
        
        // 构建API请求URL
        let url = format!(
            "https://api.hyperliquid.xyz/info/spotUserFills?user={}",
            self.address
        );
        
        // 发送请求
        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("请求API失败: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API请求失败，状态码: {}", status));
        }
        
        // 解析响应
        let data: Value = response.json()
            .await
            .map_err(|e| anyhow!("解析响应失败: {}", e))?;
            
        Ok(data)
    }
    
    /// 获取用户的合约交易历史
    async fn get_contract_trades(&self) -> Result<Value> {
        debug!("获取用户合约交易历史: {}", self.address);
        
        // 构建API请求URL
        let url = format!(
            "https://api.hyperliquid.xyz/info/userFills?user={}",
            self.address
        );
        
        // 发送请求
        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("请求API失败: {}", e))?;
            
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("API请求失败，状态码: {}", status));
        }
        
        // 解析响应
        let data: Value = response.json()
            .await
            .map_err(|e| anyhow!("解析响应失败: {}", e))?;
            
        Ok(data)
    }
    
    /// 检查用户的现货交易变化
    async fn check_spot_trades(&mut self) -> Result<Option<Change>> {
        if !self.monitor_spot {
            return Ok(None);
        }
        
        let trades = self.get_spot_trades().await?;
        
        // 检查是否有交易记录
        let trades_array = trades.as_array()
            .ok_or_else(|| anyhow!("API返回数据格式不正确"))?;
            
        if trades_array.is_empty() {
            debug!("未找到现货交易记录");
            return Ok(None);
        }
        
        // 获取最新交易记录
        let latest_trade = &trades_array[0];
        
        // 提取交易ID
        let trade_id = latest_trade["tid"].as_str()
            .ok_or_else(|| anyhow!("交易ID格式不正确"))?
            .to_string();
            
        // 检查是否有新交易
        if let Some(last_id) = &self.last_spot_trade_id {
            if last_id != &trade_id {
                // 提取交易详情
                let asset = latest_trade["asset"].as_str().unwrap_or("未知");
                let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "买入" } else { "卖出" };
                let price = latest_trade["px"].as_str().unwrap_or("0");
                let size = latest_trade["sz"].as_str().unwrap_or("0");
                let time = latest_trade["time"].as_u64().unwrap_or(0);
                
                // 构建变化通知
                let change = Change {
                    message: format!("检测到新的现货交易: {} {}", asset, side),
                    details: format!(
                        "用户: {}\n资产: {}\n方向: {}\n价格: {}\n数量: {}\n时间: {}\n交易ID: {}",
                        self.address, asset, side, price, size, time, trade_id
                    ),
                };
                
                // 更新最后交易ID
                self.last_spot_trade_id = Some(trade_id);
                
                return Ok(Some(change));
            }
        } else {
            // 首次检查，不触发变化通知
            debug!("首次获取现货交易记录");
            self.last_spot_trade_id = Some(trade_id);
        }
        
        Ok(None)
    }
    
    /// 检查用户的合约交易变化
    async fn check_contract_trades(&mut self) -> Result<Option<Change>> {
        if !self.monitor_contract {
            return Ok(None);
        }
        
        let trades = self.get_contract_trades().await?;
        
        // 检查是否有交易记录
        let trades_array = trades.as_array()
            .ok_or_else(|| anyhow!("API返回数据格式不正确"))?;
            
        if trades_array.is_empty() {
            debug!("未找到合约交易记录");
            return Ok(None);
        }
        
        // 获取最新交易记录
        let latest_trade = &trades_array[0];
        
        // 提取交易ID
        let trade_id = latest_trade["tid"].as_str()
            .ok_or_else(|| anyhow!("交易ID格式不正确"))?
            .to_string();
            
        // 检查是否有新交易
        if let Some(last_id) = &self.last_contract_trade_id {
            if last_id != &trade_id {
                // 提取交易详情
                let asset = latest_trade["coin"].as_str().unwrap_or("未知");
                let side = if latest_trade["side"].as_str().unwrap_or("") == "B" { "买入" } else { "卖出" };
                let price = latest_trade["px"].as_str().unwrap_or("0");
                let size = latest_trade["sz"].as_str().unwrap_or("0");
                let time = latest_trade["time"].as_u64().unwrap_or(0);
                
                // 构建变化通知
                let change = Change {
                    message: format!("检测到新的合约交易: {} {}", asset, side),
                    details: format!(
                        "用户: {}\n资产: {}\n方向: {}\n价格: {}\n数量: {}\n时间: {}\n交易ID: {}",
                        self.address, asset, side, price, size, time, trade_id
                    ),
                };
                
                // 更新最后交易ID
                self.last_contract_trade_id = Some(trade_id);
                
                return Ok(Some(change));
            }
        } else {
            // 首次检查，不触发变化通知
            debug!("首次获取合约交易记录");
            self.last_contract_trade_id = Some(trade_id);
        }
        
        Ok(None)
    }
}

impl Monitor for HyperliquidMonitor {
    async fn check(&mut self) -> Result<Option<Change>> {
        // 检查现货交易
        if let Some(change) = self.check_spot_trades().await? {
            return Ok(Some(change));
        }
        
        // 检查合约交易
        if let Some(change) = self.check_contract_trades().await? {
            return Ok(Some(change));
        }
        
        Ok(None)
    }
    
    fn interval(&self) -> u64 {
        self.interval_secs
    }
} 