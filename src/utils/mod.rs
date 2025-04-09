use anyhow::Result;
use log::error;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use chrono::TimeZone;

/// Write data to file
pub fn write_to_file<P: AsRef<Path>>(path: P, data: &str) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

/// Read data from file
pub fn read_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let mut file = File::open(&path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

/// Ensure directory exists, create if not
pub fn ensure_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Calculate string difference percentage
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
    
    // Use simple length comparison as difference estimate
    let diff = (new_len - old_len).abs() / old_len.max(new_len);
    
    diff * 100.0
}

/// Format timestamp to readable string
pub fn format_timestamp(timestamp_ms: u64) -> String {
    match chrono::Local.timestamp_opt(timestamp_ms as i64 / 1000, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => {
            error!("Invalid timestamp: {}", timestamp_ms);
            String::from("Time format error")
        }
    }
} 