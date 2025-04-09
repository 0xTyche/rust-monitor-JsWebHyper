use std::path::PathBuf;
use clap::{Parser, Subcommand};
use log::{info, error};
use anyhow::Result;
use dotenv::dotenv;

mod monitors;
mod notifiers;
mod utils;

use monitors::{
    js_monitor::JsMonitor,
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
    /// Monitor JS data changes
    Js {
        /// Website URL to monitor
        #[arg(short, long)]
        url: String,

        /// JS data selector
        #[arg(short, long)]
        selector: String,

        /// Monitoring interval (seconds)
        #[arg(short, long, default_value_t = 60)]
        interval: u64,
    },
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
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize environment variables and logging
    dotenv().ok();
    env_logger::init();
    
    let cli = Cli::parse();
    
    // If a configuration file is provided, load settings from it
    if let Some(config_path) = cli.config {
        info!("Loading settings from config file: {:?}", config_path);
        // TODO: Implement loading settings from config file
    }
    
    // Execute the appropriate monitoring task based on command line arguments
    match &cli.command {
        Some(Commands::Js { url, selector, interval }) => {
            info!("Starting JS data monitoring: {}", url);
            let monitor = JsMonitor::new(url, selector, *interval);
            run_monitor(monitor).await?;
        }
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