use std::path::PathBuf;
use clap::{Parser, Subcommand};
use log::{info, error, debug};
use anyhow::Result;
use dotenv::dotenv;

mod monitors;
mod notifiers;
mod utils;

use monitors::{
    static_monitor::StaticMonitor,
    hyperliquid_monitor::HyperliquidMonitor,
    Monitor
};
use notifiers::server_chan::ServerChanNotifier;
use notifiers::Notifier;

/// A tool for monitoring website data changes and Hyperliquid user transactions
#[derive(Parser)]
#[command(name = "hyperliquid_monitor")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Monitor static webpage changes
    Static {
        /// Webpage URL to monitor
        #[arg(short, long)]
        url: String,

        /// HTML selector
        #[arg(short, long)]
        selector: String,

        /// Monitoring interval (seconds)
        #[arg(short, long, default_value_t = 300)]
        interval: u64,
    },
    /// Monitor Hyperliquid user transactions
    Hyperliquid {
        /// Wallet address to monitor
        #[arg(short, long)]
        address: String,

        /// Monitoring interval (seconds)
        #[arg(short, long, default_value_t = 120)]
        interval: u64,

        /// Whether to monitor spot trading
        #[arg(long, default_value_t = true)]
        spot: bool,

        /// Whether to monitor contract trading
        #[arg(long, default_value_t = true)]
        contract: bool,
    },
    /// Monitor API data changes
    Api {
        /// API URL to monitor
        #[arg(short, long)]
        url: String,

        /// JSONPath selector
        #[arg(short, long)]
        selector: String,

        /// Monitoring interval (seconds)
        #[arg(short, long, default_value_t = 60)]
        interval: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize environment variables and logging
    dotenv().ok();
    
    // Initialize logger with debug level
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp(None)
        .format_level(true)
        .format_target(false)
        .init();
    
    info!("Starting Hyperliquid Monitor...");
    debug!("Debug logging enabled");
    
    let cli = Cli::parse();
    
    // If a configuration file is provided, load settings from it
    if let Some(config_path) = cli.config {
        info!("Loading settings from config file: {:?}", config_path);
        // TODO: Implement loading settings from config file
    }
    
    // Execute the appropriate monitoring task based on command line arguments
    match &cli.command {
        Some(Commands::Static { url, selector, interval }) => {
            info!("Starting static webpage monitoring: {}", url);
            let monitor = StaticMonitor::new(url, selector, *interval);
            run_monitor(monitor).await?;
        }
        Some(Commands::Hyperliquid { address, interval, spot, contract }) => {
            info!("Starting Hyperliquid user transaction monitoring: {}", address);
            let monitor = HyperliquidMonitor::new(address, *interval, *spot, *contract);
            run_monitor(monitor).await?;
        }
        Some(Commands::Api { url, selector, interval }) => {
            info!("Starting API data monitoring: {}", url);
            let monitor = monitors::api_monitor::ApiMonitor::new(url.clone(), selector.clone(), *interval);
            run_monitor(monitor).await?;
        }
        None => {
            // If no subcommand is specified, display help information
            println!("Please specify a monitoring command to execute. Use --help to view help information.");
        }
    }
    
    Ok(())
}

async fn run_monitor<M: Monitor>(mut monitor: M) -> Result<()> {
    // Create notification service
    let server_chan_key = std::env::var("SERVER_CHAN_KEY").unwrap_or_default();
    let notifier = ServerChanNotifier::new(&server_chan_key);
    
    // Get initial content and send initial notification
    let monitor_name = monitor.get_name();
    info!("Starting monitoring: {}", monitor_name);
    
    // First check to get initial content
    match monitor.check().await {
        Ok(Some(change)) => {
            // Already have a change on first check - unusual but possible
            info!("Initial check detected change: {}", change.message);
            
            // Send initial notification with the change details
            let initial_message = format!("Started monitoring: {}", monitor_name);
            if let Err(e) = notifier.send(&initial_message, &change.details).await {
                error!("Failed to send initial notification: {}", e);
            } else {
                info!("Initial notification sent");
            }
        },
        Ok(None) => {
            // Normal case - content captured but no change
            info!("Initial content captured for: {}", monitor_name);
            
            // Send notification about monitoring start
            let initial_message = format!("Started monitoring: {}", monitor_name);
            let details = format!("Initial content captured. Will notify when changes are detected.");
            
            if let Err(e) = notifier.send(&initial_message, &details).await {
                error!("Failed to send initial notification: {}", e);
            } else {
                info!("Initial notification sent");
            }
        },
        Err(e) => {
            // Error on first check
            error!("Error getting initial content: {}", e);
            // Continue to monitor anyway
        },
    }
    
    // Start monitoring loop
    loop {
        match monitor.check().await {
            Ok(Some(change)) => {
                info!("Change detected: {}", change.message);
                if let Err(e) = notifier.send(&change.message, &change.details).await {
                    error!("Failed to send notification: {}", e);
                }
            }
            Ok(None) => {
                info!("No changes detected");
            }
            Err(e) => {
                error!("Error during monitoring: {}", e);
            }
        }
        
        // Wait for next check
        tokio::time::sleep(std::time::Duration::from_secs(monitor.interval())).await;
    }
} 