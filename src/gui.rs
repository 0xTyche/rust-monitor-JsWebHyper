use eframe::{egui, Frame, CreationContext};
use egui::{Color32, RichText, Ui, Vec2};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use anyhow::Result;
use dotenv::dotenv;
use log::{debug, LevelFilter};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;

mod monitors;
mod notifiers;
mod utils;

use monitors::{
    js_monitor::JsMonitor,
    static_monitor::StaticMonitor,
    hyperliquid_monitor::HyperliquidMonitor,
    Monitor, Change
};
use notifiers::server_chan::ServerChanNotifier;
use notifiers::Notifier;

/// Maximum number of log entries
const MAX_LOGS: usize = 100;

/// Monitoring task status
#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Stopped,
    Running,
    Error(String),
}

/// Monitoring task type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum TaskType {
    JsMonitor,
    StaticMonitor,
    HyperliquidMonitor,
}

/// Monitoring task configuration
#[derive(Clone, Serialize, Deserialize)]
struct TaskConfig {
    /// Task name
    name: String,
    /// Task type
    task_type: TaskType,
    /// Target URL to monitor
    url: String,
    /// Selector (for JS and static web page monitoring)
    selector: String,
    /// Wallet address (for Hyperliquid monitoring)
    address: String,
    /// Whether to monitor spot trading (for Hyperliquid monitoring)
    monitor_spot: bool,
    /// Whether to monitor contract trading (for Hyperliquid monitoring)
    monitor_contract: bool,
    /// Monitoring interval (seconds)
    interval_secs: u64,
    /// Whether it's enabled
    enabled: bool,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            name: "New Task".to_string(),
            task_type: TaskType::JsMonitor,
            url: "https://example.com".to_string(),
            selector: "document.querySelector('.price')".to_string(),
            address: "".to_string(),
            monitor_spot: true,
            monitor_contract: true,
            interval_secs: 60,
            enabled: false,
        }
    }
}

/// Notification configuration
#[derive(Clone, Serialize, Deserialize)]
struct NotificationConfig {
    /// Whether to enable ServerChan notifications
    enable_server_chan: bool,
    /// ServerChan key
    server_chan_key: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enable_server_chan: false,
            server_chan_key: "".to_string(),
        }
    }
}

/// Application configuration
#[derive(Clone, Serialize, Deserialize)]
struct AppConfig {
    /// Notification configuration
    notification: NotificationConfig,
    /// Monitoring task list
    tasks: Vec<TaskConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            notification: NotificationConfig::default(),
            tasks: Vec::new(),
        }
    }
}

/// Monitoring application state
struct MonitorApp {
    /// Application configuration
    config: AppConfig,
    /// Task configuration being edited
    editing_task: TaskConfig,
    /// Whether to show the add task dialog
    show_add_task_dialog: bool,
    /// Whether to show the edit task dialog
    show_edit_task_dialog: bool,
    /// Current editing task index
    editing_task_index: Option<usize>,
    /// Current task statuses
    task_statuses: Vec<TaskStatus>,
    /// Runtime
    runtime: Runtime,
    /// Task handles
    task_handles: Vec<Option<JoinHandle<()>>>,
    /// Log records
    logs: VecDeque<(String, Color32)>,
    /// Notification service
    notifier: Option<Arc<ServerChanNotifier>>,
    /// Configuration file path
    config_path: String,
}

/// Message type
enum Message {
    Log(String, Color32),
    TaskStatusChanged(usize, TaskStatus),
    ChangeDetected(usize, Change),
}

impl MonitorApp {
    /// Create a new monitoring application
    fn new(_cc: &CreationContext) -> Self {
        // Initialize environment variables
        dotenv().ok();
        
        // Configuration file path
        let config_path = "config.json".to_string();
        
        // Try to load saved configuration
        let config = Self::load_config(&config_path).unwrap_or_default();
        
        // Create Tokio runtime
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        
        // Initialize state
        let task_statuses = vec![TaskStatus::Stopped; config.tasks.len()];
        let mut task_handles = Vec::with_capacity(config.tasks.len());
        for _ in 0..config.tasks.len() {
            task_handles.push(None);
        }
        
        // Initialize notification service
        let notifier = if !config.notification.server_chan_key.is_empty() {
            Some(Arc::new(ServerChanNotifier::new(&config.notification.server_chan_key)))
        } else {
            // Try to load ServerChan key from environment variables
            match std::env::var("SERVER_CHAN_KEY") {
                Ok(key) if !key.is_empty() => Some(Arc::new(ServerChanNotifier::new(&key))),
                _ => None,
            }
        };
        
        let mut app = Self {
            config,
            editing_task: TaskConfig::default(),
            show_add_task_dialog: false,
            show_edit_task_dialog: false,
            editing_task_index: None,
            task_statuses,
            runtime,
            task_handles,
            logs: VecDeque::with_capacity(MAX_LOGS),
            notifier,
            config_path,
        };
        
        // Add welcome logs
        app.add_log("Hyperliquid Monitoring System Started", Color32::GREEN);
        app.add_log("Version: 0.1.0", Color32::WHITE);
        
        app
    }
    
    /// Load configuration
    fn load_config(path: &str) -> Result<AppConfig> {
        let config_file = Path::new(path);
        if config_file.exists() {
            let config_str = fs::read_to_string(config_file)?;
            let config: AppConfig = serde_json::from_str(&config_str)?;
            Ok(config)
        } else {
            Err(anyhow::anyhow!("Configuration file does not exist"))
        }
    }
    
    /// Save configuration
    fn save_config(&self) -> Result<()> {
        let config_str = serde_json::to_string_pretty(&self.config)?;
        fs::write(&self.config_path, config_str)?;
        Ok(())
    }
    
    /// Add log
    fn add_log(&mut self, message: &str, color: Color32) {
        let timestamp = chrono::Local::now().format("[%H:%M:%S]").to_string();
        let log_message = format!("{} {}", timestamp, message);
        
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        
        self.logs.push_back((log_message, color));
    }
    
    /// Start monitoring task
    fn start_task(&mut self, task_index: usize) {
        if task_index >= self.config.tasks.len() {
            return;
        }
        
        // If the task is already running, stop it first
        if let Some(handle) = &self.task_handles[task_index] {
            handle.abort();
            self.task_handles[task_index] = None;
        }
        
        let task_config = self.config.tasks[task_index].clone();
        self.task_statuses[task_index] = TaskStatus::Running;
        
        // Create notification service
        let notifier = self.notifier.clone();
        
        // Create channel for sending messages
        let (tx, mut rx) = mpsc::channel::<Message>(32);
        let tx_clone = tx.clone();
        
        // Create task
        let handle = self.runtime.spawn(async move {
            match task_config.task_type {
                TaskType::JsMonitor => {
                    let mut monitor = JsMonitor::new(&task_config.url, &task_config.selector, task_config.interval_secs);
                    run_monitor_task(task_index, &mut monitor, notifier, tx).await;
                },
                TaskType::StaticMonitor => {
                    let mut monitor = StaticMonitor::new(&task_config.url, &task_config.selector, task_config.interval_secs);
                    run_monitor_task(task_index, &mut monitor, notifier, tx).await;
                },
                TaskType::HyperliquidMonitor => {
                    let mut monitor = HyperliquidMonitor::new(
                        &task_config.address,
                        task_config.interval_secs,
                        task_config.monitor_spot,
                        task_config.monitor_contract
                    );
                    run_monitor_task(task_index, &mut monitor, notifier, tx).await;
                },
            }
        });
        
        self.task_handles[task_index] = Some(handle);
        
        // Add log
        self.add_log(&format!("Started task #{}: {}", task_index + 1, task_config.name), Color32::GREEN);
        
        // Create message receiving task
        self.runtime.spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    Message::Log(message, _color) => {
                        // Handle log message
                        debug!("{}", message);
                        // Cannot directly modify application state, send back to main thread
                    },
                    Message::TaskStatusChanged(idx, status) => {
                        // Handle task status change
                        let _ = tx_clone.send(Message::Log(
                            format!("Task #{} status changed to: {:?}", idx + 1, status),
                            match status {
                                TaskStatus::Running => Color32::GREEN,
                                TaskStatus::Stopped => Color32::YELLOW,
                                TaskStatus::Error(_) => Color32::RED,
                            }
                        )).await;
                    },
                    Message::ChangeDetected(idx, change) => {
                        // Handle detected change
                        let _ = tx_clone.send(Message::Log(
                            format!("Task #{} detected change: {}", idx + 1, change.message),
                            Color32::GOLD
                        )).await;
                    },
                }
            }
        });
    }
    
    /// Stop monitoring task
    fn stop_task(&mut self, task_index: usize) {
        if task_index >= self.config.tasks.len() {
            return;
        }
        
        if let Some(handle) = &self.task_handles[task_index] {
            handle.abort();
            self.task_handles[task_index] = None;
            self.task_statuses[task_index] = TaskStatus::Stopped;
            
            // Add log
            self.add_log(&format!("Stopped task #{}: {}", task_index + 1, self.config.tasks[task_index].name), Color32::YELLOW);
        }
    }
    
    /// Stop all tasks
    fn stop_all_tasks(&mut self) {
        for i in 0..self.task_handles.len() {
            self.stop_task(i);
        }
    }
    
    /// Add new task
    fn add_task(&mut self) {
        self.config.tasks.push(self.editing_task.clone());
        self.task_statuses.push(TaskStatus::Stopped);
        self.task_handles.push(None);
        
        // Add log
        self.add_log(&format!("Added new task: {}", self.editing_task.name), Color32::LIGHT_BLUE);
        
        // Save configuration
        if let Err(e) = self.save_config() {
            self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
        }
        
        // Reset edit state
        self.editing_task = TaskConfig::default();
        self.show_add_task_dialog = false;
    }
    
    /// Update task
    fn update_task(&mut self) {
        if let Some(idx) = self.editing_task_index {
            if idx < self.config.tasks.len() {
                // If task is running, stop it first
                self.stop_task(idx);
                
                // Update task configuration
                self.config.tasks[idx] = self.editing_task.clone();
                
                // Add log
                self.add_log(&format!("Updated task #{}: {}", idx + 1, self.editing_task.name), Color32::LIGHT_BLUE);
                
                // Save configuration
                if let Err(e) = self.save_config() {
                    self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
                }
            }
        }
        
        // Reset edit state
        self.editing_task = TaskConfig::default();
        self.show_edit_task_dialog = false;
        self.editing_task_index = None;
    }
    
    /// Delete task
    fn delete_task(&mut self, task_index: usize) {
        if task_index < self.config.tasks.len() {
            // If task is running, stop it first
            self.stop_task(task_index);
            
            // Add log
            self.add_log(&format!("Deleted task: {}", self.config.tasks[task_index].name), Color32::LIGHT_RED);
            
            // Delete task
            self.config.tasks.remove(task_index);
            self.task_statuses.remove(task_index);
            self.task_handles.remove(task_index);
            
            // Save configuration
            if let Err(e) = self.save_config() {
                self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
            }
        }
    }
    
    /// Update notification settings
    fn update_notification_config(&mut self) {
        // Update notification service
        if self.config.notification.enable_server_chan && !self.config.notification.server_chan_key.is_empty() {
            self.notifier = Some(Arc::new(ServerChanNotifier::new(&self.config.notification.server_chan_key)));
            self.add_log("Updated ServerChan notification configuration", Color32::LIGHT_BLUE);
        } else {
            self.notifier = None;
            self.add_log("Disabled ServerChan notifications", Color32::YELLOW);
        }
        
        // Save configuration
        if let Err(e) = self.save_config() {
            self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
        }
    }
    
    /// Draw main UI
    fn draw_main_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Add Task").clicked() {
                self.editing_task = TaskConfig::default();
                self.show_add_task_dialog = true;
            }
            
            if ui.button("Save Configuration").clicked() {
                if let Err(e) = self.save_config() {
                    self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
                } else {
                    self.add_log("Configuration saved", Color32::GREEN);
                }
            }
        });
        
        ui.separator();
        
        // Notification configuration
        ui.collapsing("Notification Settings", |ui| {
            ui.checkbox(&mut self.config.notification.enable_server_chan, "Enable ServerChan Notifications");
            
            if self.config.notification.enable_server_chan {
                ui.horizontal(|ui| {
                    ui.label("ServerChan Key:");
                    ui.text_edit_singleline(&mut self.config.notification.server_chan_key);
                });
                
                if ui.button("Update Notification Settings").clicked() {
                    self.update_notification_config();
                }
            }
        });
        
        ui.separator();
        
        // Task list
        ui.heading("Task List");
        
        let task_count = self.config.tasks.len();
        if task_count == 0 {
            ui.label("No tasks yet. Click 'Add Task' button to add monitoring tasks");
        } else {
            self.draw_task_list(ui);
        }
        
        ui.separator();
        
        // Log area
        ui.heading("Logs");
        egui::ScrollArea::vertical().max_height(200.0).stick_to_bottom(true).show(ui, |ui| {
            for (log, color) in &self.logs {
                ui.label(RichText::new(log).color(*color));
            }
        });
    }
    
    /// Draw add task dialog
    fn draw_add_task_dialog(&mut self, ctx: &egui::Context) {
        let mut show_dialog = self.show_add_task_dialog;
        let mut add_task = false;
        let mut cancel = false;
        
        if show_dialog {
            egui::Window::new("Add Monitoring Task")
                .resizable(true)
                .fixed_size(Vec2::new(400.0, 400.0))
                .open(&mut show_dialog)
                .show(ctx, |ui| {
                    self.draw_task_form(ui);
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        add_task = ui.button("Add").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
        }
        
        // Handle button clicks outside of the dialog
        if add_task {
            self.add_task();
        } else if cancel {
            show_dialog = false;
        }
        
        self.show_add_task_dialog = show_dialog;
    }
    
    /// Draw edit task dialog
    fn draw_edit_task_dialog(&mut self, ctx: &egui::Context) {
        let mut show_dialog = self.show_edit_task_dialog;
        let mut update_task = false;
        let mut cancel = false;
        
        if show_dialog {
            egui::Window::new("Edit Monitoring Task")
                .resizable(true)
                .fixed_size(Vec2::new(400.0, 400.0))
                .open(&mut show_dialog)
                .show(ctx, |ui| {
                    self.draw_task_form(ui);
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        update_task = ui.button("Update").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
        }
        
        // Handle button clicks outside of the dialog
        if update_task {
            self.update_task();
        } else if cancel {
            show_dialog = false;
            self.editing_task_index = None;
        }
        
        self.show_edit_task_dialog = show_dialog;
    }
    
    /// Draw task form
    fn draw_task_form(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Task Name:");
            ui.text_edit_singleline(&mut self.editing_task.name);
        });
        
        ui.horizontal(|ui| {
            ui.label("Task Type:");
            ui.radio_value(&mut self.editing_task.task_type, TaskType::JsMonitor, "JS Data Monitor");
            ui.radio_value(&mut self.editing_task.task_type, TaskType::StaticMonitor, "Static Webpage Monitor");
            ui.radio_value(&mut self.editing_task.task_type, TaskType::HyperliquidMonitor, "Hyperliquid Monitor");
        });
        
        ui.horizontal(|ui| {
            ui.label("Monitoring Interval (seconds):");
            ui.add(egui::Slider::new(&mut self.editing_task.interval_secs, 5..=3600));
        });
        
        match self.editing_task.task_type {
            TaskType::JsMonitor | TaskType::StaticMonitor => {
                ui.horizontal(|ui| {
                    ui.label("URL to Monitor:");
                    ui.text_edit_singleline(&mut self.editing_task.url);
                });
                
                ui.horizontal(|ui| {
                    ui.label("Selector:");
                    ui.text_edit_singleline(&mut self.editing_task.selector);
                });
                
                if self.editing_task.task_type == TaskType::JsMonitor {
                    ui.label("JS Selector Example: document.querySelector('.price').textContent");
                } else {
                    ui.label("CSS Selector Example: #price-container .value");
                }
            },
            TaskType::HyperliquidMonitor => {
                ui.horizontal(|ui| {
                    ui.label("Wallet Address:");
                    ui.text_edit_singleline(&mut self.editing_task.address);
                });
                
                ui.checkbox(&mut self.editing_task.monitor_spot, "Monitor Spot Trading");
                ui.checkbox(&mut self.editing_task.monitor_contract, "Monitor Contract Trading");
            },
        }
    }
    
    /// Draw task list
    fn draw_task_list(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let task_count = self.config.tasks.len();
            
            for i in 0..task_count {
                let task_clone = self.config.tasks[i].clone();
                let status = self.task_statuses[i].clone();
                
                ui.horizontal(|ui| {
                    let status_text = match &status {
                        TaskStatus::Running => RichText::new("⚡ Running").color(Color32::GREEN),
                        TaskStatus::Stopped => RichText::new("⏹ Stopped").color(Color32::YELLOW),
                        TaskStatus::Error(e) => RichText::new(format!("❌ Error: {}", e)).color(Color32::RED),
                    };
                    
                    ui.label(format!("#{}: ", i + 1));
                    ui.label(RichText::new(&task_clone.name).strong());
                    ui.label(status_text);
                    
                    let is_running = matches!(status, TaskStatus::Running);
                    
                    if is_running {
                        if ui.button("Stop").clicked() {
                            self.stop_task(i);
                        }
                    } else {
                        if ui.button("Start").clicked() {
                            self.start_task(i);
                        }
                    }
                    
                    if ui.button("Edit").clicked() {
                        self.editing_task = task_clone.clone();
                        self.editing_task_index = Some(i);
                        self.show_edit_task_dialog = true;
                    }
                    
                    if ui.button("Delete").clicked() {
                        self.delete_task(i);
                        return;
                    }
                });
                
                ui.horizontal(|ui| {
                    match task_clone.task_type {
                        TaskType::JsMonitor => {
                            ui.label(format!("Type: JS Monitor | URL: {} | Selector: {} | Interval: {}s", 
                                       task_clone.url, task_clone.selector, task_clone.interval_secs));
                        },
                        TaskType::StaticMonitor => {
                            ui.label(format!("Type: Static Webpage Monitor | URL: {} | Selector: {} | Interval: {}s", 
                                       task_clone.url, task_clone.selector, task_clone.interval_secs));
                        },
                        TaskType::HyperliquidMonitor => {
                            ui.label(format!("Type: Hyperliquid Monitor | Address: {} | Spot: {} | Contract: {} | Interval: {}s", 
                                       task_clone.address, 
                                       if task_clone.monitor_spot { "Yes" } else { "No" }, 
                                       if task_clone.monitor_contract { "Yes" } else { "No" }, 
                                       task_clone.interval_secs));
                        },
                    }
                });
                
                ui.separator();
            }
        });
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hyperliquid Monitoring System");
            ui.separator();
            
            self.draw_main_ui(ui);
        });
        
        if self.show_add_task_dialog {
            self.draw_add_task_dialog(ctx);
        }
        
        if self.show_edit_task_dialog {
            self.draw_edit_task_dialog(ctx);
        }
        
        // Refresh UI every second
        ctx.request_repaint_after(Duration::from_secs(1));
    }
    
    // 添加 on_exit 方法，在应用退出时停止所有任务
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Stop all tasks
        self.stop_all_tasks();
        
        // Save configuration
        if let Err(e) = self.save_config() {
            eprintln!("Failed to save configuration: {}", e);
        }
    }
}

/// Run monitoring task
async fn run_monitor_task<M: Monitor>(
    task_index: usize, 
    monitor: &mut M, 
    notifier: Option<Arc<ServerChanNotifier>>,
    tx: mpsc::Sender<Message>
) {
    let interval_secs = monitor.interval();
    
    // Send task start message
    let _ = tx.send(Message::TaskStatusChanged(task_index, TaskStatus::Running)).await;
    
    loop {
        match monitor.check().await {
            Ok(Some(change)) => {
                // Send change detection message
                let _ = tx.send(Message::ChangeDetected(task_index, change.clone())).await;
                
                // Send notification
                if let Some(notifier) = &notifier {
                    if let Err(e) = notifier.send(&change.message, &change.details).await {
                        let _ = tx.send(Message::Log(
                            format!("Failed to send notification: {}", e),
                            Color32::RED
                        )).await;
                    }
                }
            },
            Ok(None) => {
                // No change
                let _ = tx.send(Message::Log(
                    format!("Task #{} detected no changes", task_index + 1),
                    Color32::GRAY
                )).await;
            },
            Err(e) => {
                // Send error message
                let _ = tx.send(Message::TaskStatusChanged(
                    task_index,
                    TaskStatus::Error(e.to_string())
                )).await;
                
                // Wait for a while before retrying
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
                continue;
            },
        }
        
        // Wait for next check
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

fn main() -> Result<(), eframe::Error> {
    // Set up logging
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .init();
    
    // Run eframe application with updated options
    let native_options = eframe::NativeOptions {
        initial_window_size: Some(Vec2::new(800.0, 600.0)),
        min_window_size: Some(Vec2::new(600.0, 400.0)),
        follow_system_theme: true,
        default_theme: eframe::Theme::Light,
        // 移除不兼容的选项
        ..Default::default()
    };
    
    eframe::run_native(
        "Hyperliquid Monitoring System",
        native_options,
        Box::new(|cc| Box::new(MonitorApp::new(cc)))
    )
} 