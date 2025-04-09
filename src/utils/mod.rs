use anyhow::Result;
use log::error;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use chrono::TimeZone;

/// 将数据写入文件
pub fn write_to_file<P: AsRef<Path>>(path: P, data: &str) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

/// 从文件读取数据
pub fn read_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let mut file = File::open(&path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

/// 确保目录存在，如果不存在则创建
pub fn ensure_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// 计算字符串差异百分比
pub fn diff_percentage(old_str: &str, new_str: &str) -> f64 {
    if old_str.is_empty() && new_str.is_empty() {
        return 0.0;
    }
    
    if old_str.is_empty() {
        return 100.0;
    }
    
    if new_str.is_empty() {
        return 100.0;
    }
    
    let old_len = old_str.len() as f64;
    let new_len = new_str.len() as f64;
    
    // 使用简单的长度比较作为差异估计
    let diff = (new_len - old_len).abs() / old_len.max(new_len);
    
    diff * 100.0
}

/// 格式化时间戳为可读字符串
pub fn format_timestamp(timestamp_ms: u64) -> String {
    match chrono::Local.timestamp_opt(timestamp_ms as i64 / 1000, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => {
            error!("无效的时间戳: {}", timestamp_ms);
            String::from("时间格式错误")
        }
    }
} 