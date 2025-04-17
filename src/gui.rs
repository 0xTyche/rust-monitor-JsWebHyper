use eframe::{egui, Frame, CreationContext};
use egui::{Color32, RichText, Ui, Vec2};
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
use std::collections::HashMap;
use std::fmt;

mod monitors;
mod notifiers;
mod utils;

use monitors::{
    static_monitor::StaticMonitor,
    hyperliquid_monitor::HyperliquidMonitor,
    api_monitor::ApiMonitor,
    Monitor, Change
};
use notifiers::server_chan::ServerChanNotifier;
use notifiers::Notifier;

/// Maximum number of log entries
const MAX_LOGS: usize = 100;

/// Monitoring task status
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TaskStatus {
    Idle,
    Running,
    Error,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Idle => write!(f, "Idle"),
            TaskStatus::Running => write!(f, "Running"),
            TaskStatus::Error => write!(f, "Error"),
        }
    }
}

/// Monitoring task type
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TaskType {
    Static,
    Hyperliquid,
    Api,
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskType::Static => write!(f, "Static Web"),
            TaskType::Hyperliquid => write!(f, "Hyperliquid"),
            TaskType::Api => write!(f, "API Monitor"),
        }
    }
}

impl TaskType {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Static Web" => Some(TaskType::Static),
            "Hyperliquid" => Some(TaskType::Hyperliquid),
            "API Monitor" => Some(TaskType::Api),
            _ => None,
        }
    }
}

/// Monitoring task configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskConfig {
    /// Task name
    pub name: String,
    /// Task type
    pub task_type: String,
    /// Target URL to monitor
    pub url: String,
    /// Selector (for static web page monitoring and API)
    pub selector: String,
    /// Wallet address (for Hyperliquid monitoring)
    pub address: String,
    /// Whether to monitor spot trading (for Hyperliquid monitoring)
    pub monitor_spot: bool,
    /// Whether to monitor contract trading (for Hyperliquid monitoring)
    pub monitor_contract: bool,
    /// Monitoring interval (seconds)
    pub interval_secs: u64,
    /// Whether it's enabled
    pub enabled: bool,
    /// Notes for the task
    pub notes: String,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            name: "New Task".to_string(),
            task_type: "Static Web".to_string(),
            url: "https://example.com".to_string(),
            selector: "".to_string(),
            address: "".to_string(),
            monitor_spot: true,
            monitor_contract: false,
            interval_secs: 60,
            enabled: true,
            notes: String::new(),
        }
    }
}

/// Notification configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationConfig {
    /// Whether to enable ServerChan notifications
    pub enabled: bool,
    /// ServerChan key
    pub server_chan_key: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_chan_key: String::new(),
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Configs {
    /// Notification configuration
    pub notification: NotificationConfig,
    /// Monitoring task list
    pub tasks: Vec<TaskConfig>,
}

impl Default for Configs {
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
    configs: Configs,
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
    /// Task states
    task_states: HashMap<String, TaskStatus>,
    /// Notification sender
    notification_sender: Option<tokio::sync::mpsc::UnboundedSender<(String, String)>>,
    /// Notification handle
    notification_handle: Option<JoinHandle<()>>,
    /// Selected task
    selected_task: Option<String>,
    /// New task
    new_task: TaskConfig,
    /// Edit task
    edit_task: TaskConfig,
    /// Notifier logs
    notifier_logs: Vec<String>,
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
        let task_statuses = vec![TaskStatus::Idle; config.tasks.len()];
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
            configs: config,
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
            task_states: HashMap::new(),
            notification_sender: None,
            notification_handle: None,
            selected_task: None,
            new_task: TaskConfig::default(),
            edit_task: TaskConfig::default(),
            notifier_logs: Vec::new(),
        };
        
        // Add welcome logs
        app.add_log("Hyperliquid Monitoring System Started", Color32::GREEN);
        app.add_log("Version: 0.1.0", Color32::WHITE);
        
        app
    }
    
    /// Load configuration
    fn load_config(path: &str) -> Result<Configs> {
        let config_file = Path::new(path);
        if config_file.exists() {
            let config_str = fs::read_to_string(config_file)?;
            let config: Configs = serde_json::from_str(&config_str)?;
            
            Ok(config)
        } else {
            Err(anyhow::anyhow!("Configuration file does not exist"))
        }
    }
    
    /// Save configuration
    fn save_config(&self) -> Result<()> {
        let config_str = serde_json::to_string_pretty(&self.configs)?;
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
        if task_index >= self.configs.tasks.len() {
            return;
        }
        
        // If the task is already running, stop it first
        if let Some(handle) = &self.task_handles[task_index] {
            handle.abort();
            self.task_handles[task_index] = None;
        }
        
        let task_config = self.configs.tasks[task_index].clone();
        
        self.task_statuses[task_index] = TaskStatus::Running;
        
        // Create notification service
        let notifier = self.notifier.clone();
        
        // Create channel for sending messages
        let (tx, mut rx) = mpsc::channel::<Message>(32);
        let tx_clone = tx.clone();
        
        // Record task name for later use
        let task_name = task_config.name.clone();
        
        // Create monitor based on task type
        let monitor: Box<dyn Monitor> = match task_config.task_type.as_str() {
            "Static" => {
                Box::new(StaticMonitor::new_with_notes(
                    &task_config.url,
                    &task_config.selector,
                    task_config.interval_secs,
                    &task_config.notes,
                ))
            }
            "Api" => {
                Box::new(ApiMonitor::new_with_notes(
                    task_config.url,
                    task_config.selector,
                    task_config.interval_secs,
                    &task_config.notes,
                ))
            }
            "Hyperliquid" => {
                Box::new(HyperliquidMonitor::new_with_notes(
                    &task_config.address,
                    task_config.interval_secs,
                    task_config.monitor_spot,
                    task_config.monitor_contract,
                    &task_config.notes,
                ))
            }
            _ => {
                self.add_log(&format!("Unknown task type: {}", task_config.task_type), Color32::RED);
                return;
            }
        };
        
        // Create monitoring task
        let handle = self.runtime.spawn(async move {
            run_monitor_task(task_index, monitor, notifier, tx).await;
        });
        
        self.task_handles[task_index] = Some(handle);
        
        // Add log - Using the previously saved task_name instead of the moved task_config
        self.add_log(&format!("Started task #{}: {}", task_index + 1, task_name), Color32::GREEN);
        
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
                                TaskStatus::Idle => Color32::YELLOW,
                                TaskStatus::Error => Color32::RED,
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
        if task_index >= self.configs.tasks.len() {
            return;
        }
        
        if let Some(handle) = &self.task_handles[task_index] {
            handle.abort();
            self.task_handles[task_index] = None;
            self.task_statuses[task_index] = TaskStatus::Idle;
            
            // Add log
            self.add_log(&format!("Stopped task #{}: {}", task_index + 1, self.configs.tasks[task_index].name), Color32::YELLOW);
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
        self.configs.tasks.push(self.editing_task.clone());
        self.task_statuses.push(TaskStatus::Idle);
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
        if let Some(_idx) = self.editing_task_index {
            if _idx < self.configs.tasks.len() {
                // If task is running, stop it first
                self.stop_task(_idx);
                
                // Update task configuration
                self.configs.tasks[_idx] = self.editing_task.clone();
                
                // Add log
                self.add_log(&format!("Updated task #{}: {}", _idx + 1, self.editing_task.name), Color32::LIGHT_BLUE);
                
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
    fn delete_task(&mut self, task_index: usize) -> bool {
        if task_index < self.configs.tasks.len() {
            // If task is running, stop it first
            self.stop_task(task_index);
            
            // Add log
            self.add_log(&format!("Deleted task: {}", self.configs.tasks[task_index].name), Color32::LIGHT_RED);
            
            // Delete task
            self.configs.tasks.remove(task_index);
            self.task_statuses.remove(task_index);
            self.task_handles.remove(task_index);
            
            // Save configuration
            if let Err(e) = self.save_config() {
                self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
            }
            
            return true;
        }
        false
    }
    
    /// Update notification settings
    fn update_notification_config(&mut self) {
        // Update notification service
        if self.configs.notification.enabled && !self.configs.notification.server_chan_key.is_empty() {
            self.notifier = Some(Arc::new(ServerChanNotifier::new(&self.configs.notification.server_chan_key)));
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
        // Top operation bar
        ui.horizontal(|ui| {
            let add_btn = ui.add_sized([120.0, 30.0], egui::Button::new("Add Task"));
            if add_btn.clicked() {
                self.editing_task = TaskConfig::default();
                self.editing_task_index = None;
                self.show_add_task_dialog = true;
            }
            
            ui.add_space(10.0);
            
            let save_btn = ui.add_sized([150.0, 30.0], egui::Button::new("Save Configuration"));
            if save_btn.clicked() {
                if let Err(e) = self.save_config() {
                    self.add_log(&format!("Failed to save configuration: {}", e), Color32::RED);
                } else {
                    self.add_log("Configuration saved", Color32::GREEN);
                }
            }
        });
        
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Notification configuration area
        ui.collapsing("Notification Settings", |ui| {
            ui.add_space(5.0);
            
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.configs.notification.enabled, "Enable ServerChan Notifications");
            });
            
            if self.configs.notification.enabled {
                ui.add_space(5.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([120.0, 24.0], egui::Label::new("ServerChan Key:"));
                    ui.add_sized([250.0, 24.0], egui::TextEdit::singleline(&mut self.configs.notification.server_chan_key)
                        .hint_text("Enter your ServerChan key")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                if ui.button("Update Notification Settings").clicked() {
                    self.update_notification_config();
                }
            }
        });
        
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Calculate available height for panels and adjust
        let available_height = ui.available_height();
        let task_list_height = available_height * 0.5; // 50% of available height
        let logs_height = available_height * 0.4; // 40% of available height
        
        // Task list
        ui.heading("Task List");
        ui.add_space(5.0);
        
        let task_count = self.configs.tasks.len();
        if task_count == 0 {
            ui.label("No tasks yet. Click 'Add Task' button to add monitoring tasks");
        } else {
            // Task list scroll area with fixed height
            egui::ScrollArea::vertical()
                .id_source("task_list_scroll_area")
                .min_scrolled_height(200.0)
                .max_height(task_list_height)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    self.draw_task_list(ui);
                });
        }
        
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Log area
        ui.heading("Logs");
        ui.add_space(5.0);
        
        // Create a combined log text for copying
        let log_text = self.logs.iter()
            .map(|(log, _color)| log.clone())
            .collect::<Vec<String>>()
            .join("\n");
        
        // Logs scroll area with fixed height    
        egui::ScrollArea::vertical()
            .id_source("logs_scroll_area")
            .min_scrolled_height(150.0)
            .max_height(logs_height)
            .stick_to_bottom(true)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // Display selectable plain text area for copying
                ui.collapsing("Logs as Plain Text (for copying)", |ui| {
                    // Add a code block with the log text - this is selectable by default
                    ui.add_sized(
                        [ui.available_width(), 150.0],
                        egui::Label::new(egui::RichText::new(&log_text).monospace())
                    );
                    ui.label("(Click and drag to select text, then Copy with Ctrl+C)");
                });
                
                ui.add_space(5.0);
                
                // Display the colored logs
                ui.label("Logs with Colored Formatting:");
                ui.add_space(5.0);
                
                for (log, color) in &self.logs {
                    ui.label(RichText::new(log).color(*color));
                }
            });
    }
    
    /// Draw add task dialog
    fn draw_add_task_dialog(&mut self, ctx: &egui::Context) {
        let mut show_dialog = self.show_add_task_dialog;
        
        if show_dialog {
            egui::Window::new("Add Monitoring Task")
                .resizable(false)
                .fixed_size(Vec2::new(450.0, 400.0))
                .open(&mut show_dialog)
                .show(ctx, |ui| {
                    self.draw_task_form(ui);
                });
        }
        
        self.show_add_task_dialog = show_dialog;
    }
    
    /// Draw edit task dialog
    fn draw_edit_task_dialog(&mut self, ctx: &egui::Context) {
        let mut show_dialog = self.show_edit_task_dialog;
        
        if show_dialog {
            egui::Window::new("Edit Monitoring Task")
                .resizable(false)
                .fixed_size(Vec2::new(450.0, 400.0))
                .open(&mut show_dialog)
                .show(ctx, |ui| {
                    self.draw_task_form(ui);
                });
        }
        
        self.show_edit_task_dialog = show_dialog;
    }
    
    /// Draw task form
    fn draw_task_form(&mut self, ui: &mut Ui) {
        // Define unified input field width
        let input_width = 250.0;
        let label_width = 120.0;
        
        ui.add_space(10.0); // Top margin
        
        // Set window title
        let is_edit_mode = self.editing_task_index.is_some();
        match self.editing_task.task_type.as_str() {
            "Static Web" => {
                if is_edit_mode {
                    ui.heading("Edit Web Monitor");
                } else {
                    ui.heading("Add Web Monitor");
                }
            },
            "API Monitor" => {
                if is_edit_mode {
                    ui.heading("Edit API Monitor");
                } else {
                    ui.heading("Add API Monitor");
                }
            },
            "Hyperliquid" => {
                if is_edit_mode {
                    ui.heading("Edit Hyperliquid Monitor");
                } else {
                    ui.heading("Add Hyperliquid Monitor");
                }
            }
            _ => {}
        }
        ui.add_space(20.0);
        
        // Monitor type selection
        ui.label("Monitor Type:");
        ui.horizontal(|ui| {
            if ui.radio_value(&mut self.editing_task.task_type, "Static Web".to_string(), "Static Web").clicked() {
                // Reset relevant fields when switching to Web monitor type
                if !is_edit_mode {
                    self.editing_task.selector = "".to_string();
                }
            }
            if ui.radio_value(&mut self.editing_task.task_type, "API Monitor".to_string(), "API Monitor").clicked() {
                // Reset relevant fields when switching to API monitor type
                if !is_edit_mode {
                    self.editing_task.selector = "$.data.price".to_string();
                }
            }
            if ui.radio_value(&mut self.editing_task.task_type, "Hyperliquid".to_string(), "Hyperliquid").clicked() {
                // Reset relevant fields when switching to Hyperliquid monitor type
                if !is_edit_mode {
                    self.editing_task.address = "0x...".to_string();
                }
            }
        });
        
        ui.add_space(15.0);
        
        // Display different form fields based on task type
        match self.editing_task.task_type.as_str() {
            "Static Web" => {
                // Web monitor form
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Website URL:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.url)
                        .hint_text("https://example.com")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("ServerChan Key:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.configs.notification.server_chan_key)
                        .hint_text("ServerChan API key")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Notes:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.notes)
                        .hint_text("Optional notes")
                        .margin(egui::vec2(8.0, 4.0)));
                });
            },
            "API Monitor" => {
                // API monitor form
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("API URL:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.url)
                        .hint_text("https://api.example.com/data")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("JSONPath:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.selector)
                        .hint_text("$.data.price (leave empty to monitor entire response)")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("ServerChan Key:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.configs.notification.server_chan_key)
                        .hint_text("ServerChan API key")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Notes:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.notes)
                        .hint_text("Optional notes")
                        .margin(egui::vec2(8.0, 4.0)));
                });
            },
            "Hyperliquid" => {
                // Hyperliquid monitor form
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Wallet Address:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.address)
                        .hint_text("0x...")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("ServerChan Key:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.configs.notification.server_chan_key)
                        .hint_text("ServerChan API key")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(10.0);
                
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Notes:"));
                    ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut self.editing_task.notes)
                        .hint_text("Optional notes")
                        .margin(egui::vec2(8.0, 4.0)));
                });
                
                ui.add_space(15.0);
                
                // Monitor options
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new("Monitor Options:"));
                    ui.vertical(|ui| {
                        ui.checkbox(&mut self.editing_task.monitor_contract, "Contract");
                        ui.checkbox(&mut self.editing_task.monitor_spot, "Spot");
                    });
                });
            },
            _ => {}
        }
        
        // Monitor interval settings
        ui.add_space(15.0);
        ui.horizontal(|ui| {
            ui.add_sized([label_width, 24.0], egui::Label::new("Interval (sec):"));
            ui.add_sized([input_width, 24.0], egui::Slider::new(&mut self.editing_task.interval_secs, 1..=3600)
                .clamp_to_range(true)
                .suffix(" sec"));
        });
        
        ui.add_space(20.0);
        
        // Add separator and bottom buttons
        ui.separator();
        ui.add_space(10.0);
        
        // Button area, right-aligned
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn_size = egui::Vec2::new(100.0, 32.0);
            
            if is_edit_mode {
                if ui.add_sized(btn_size, egui::Button::new("Update")).clicked() {
                    // Update task
                    if let Some(_idx) = self.editing_task_index {
                        self.update_task();
                    }
                }
            } else {
                if ui.add_sized(btn_size, egui::Button::new("Start Monitor")).clicked() {
                    // Create new task and start monitoring
                    self.add_task();
                    
                    // Get the index of newly added task
                    let task_index = self.configs.tasks.len() - 1;
                    self.start_task(task_index);
                    
                    // Close dialog
                    self.show_add_task_dialog = false;
                }
            }
            
            ui.add_space(10.0);
            
            if ui.add_sized(btn_size, egui::Button::new("Cancel")).clicked() {
                // Cancel operation
                self.show_add_task_dialog = false;
                self.show_edit_task_dialog = false;
                self.editing_task_index = None;
            }
        });
    }
    
    /// Draw task list
    fn draw_task_list(&mut self, ui: &mut Ui) {
        // Clone tasks to avoid borrow checker issues
        let tasks = self.configs.tasks.clone();
        let task_count = tasks.len();
        
        // 记录要删除的任务索引
        let mut delete_index: Option<usize> = None;
        
        for i in 0..task_count {
            // 确保索引仍然有效
            if i >= self.configs.tasks.len() {
                break;
            }
            
            let task_clone = self.configs.tasks[i].clone();
            let status = self.task_statuses[i].clone();
            
            // Task card style
            egui::Frame::none()
                .fill(ui.visuals().extreme_bg_color)
                .inner_margin(egui::style::Margin::symmetric(10.0, 10.0))
                .show(ui, |ui| {
                    // Top row: task name and status
                    ui.horizontal(|ui| {
                        let status_text = match &status {
                            TaskStatus::Running => RichText::new("⚡ Running").color(Color32::GREEN),
                            TaskStatus::Idle => RichText::new("⏹ Stopped").color(Color32::YELLOW),
                            TaskStatus::Error => RichText::new("❌ Error").color(Color32::RED),
                        };
                        
                        ui.label(format!("#{}: ", i + 1));
                        ui.add(egui::Label::new(RichText::new(&task_clone.name).strong().size(16.0)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(status_text);
                        });
                    });
                    
                    ui.add_space(5.0);
                    
                    // Task details
                    ui.horizontal(|ui| {
                        match task_clone.task_type.as_str() {
                            "Static Web" => {
                                ui.label(format!("Type: Static Web Monitor | URL: {} | Interval: {}s", 
                                           task_clone.url, task_clone.interval_secs));
                            },
                            "API Monitor" => {
                                ui.label(format!("Type: API Monitor | URL: {} | JSONPath: {} | Interval: {}s", 
                                           task_clone.url, task_clone.selector, task_clone.interval_secs));
                            },
                            "Hyperliquid" => {
                                ui.label(format!("Type: Hyperliquid Monitor | Address: {} | Spot: {} | Contract: {} | Interval: {}s", 
                                           task_clone.address, 
                                           if task_clone.monitor_spot { "Yes" } else { "No" }, 
                                           if task_clone.monitor_contract { "Yes" } else { "No" }, 
                                           task_clone.interval_secs));
                            },
                            _ => {}
                        }
                    });
                    
                    ui.add_space(5.0);
                    
                    // Operation buttons
                    ui.horizontal(|ui| {
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
                        
                        ui.add_space(5.0);
                        
                        if ui.button("Edit").clicked() {
                            self.editing_task = task_clone.clone();
                            self.editing_task_index = Some(i);
                            self.show_edit_task_dialog = true;
                        }
                        
                        ui.add_space(5.0);
                        
                        // 不直接删除，而是记录要删除的索引
                        if ui.button("Delete").clicked() {
                            delete_index = Some(i);
                        }
                    });
                });
            
            ui.add_space(8.0); // Space between cards
        }
        
        // 在渲染循环之后执行删除操作
        if let Some(index) = delete_index {
            self.delete_task(index);
        }
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // Set overall style
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.window_margin = egui::style::Margin::same(16.0);
        ctx.set_style(style);
        
        // Main panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("Hyperliquid Monitoring System").size(24.0));
            });
            
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(15.0);
            
            self.draw_main_ui(ui);
        });
        
        // Display task add/edit dialog
        if self.show_add_task_dialog {
            self.draw_add_task_dialog(ctx);
        }
        
        if self.show_edit_task_dialog {
            self.draw_edit_task_dialog(ctx);
        }
        
        // Refresh UI every second
        ctx.request_repaint_after(Duration::from_secs(1));
    }
    
    // Add on_exit method to stop all tasks when the application exits
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
async fn run_monitor_task<M: Monitor + ?Sized>(
    task_index: usize, 
    mut monitor: Box<M>, 
    notifier: Option<Arc<ServerChanNotifier>>,
    tx: mpsc::Sender<Message>
) {
    let interval_secs = monitor.interval();
    
    // Send task start message
    let _ = tx.send(Message::TaskStatusChanged(task_index, TaskStatus::Running)).await;
    
    // Get initial content and send initial notification
    match monitor.check().await {
        Ok(Some(change)) => {
            // This is unusual - we already have a change on first check
            // Still, we'll treat it as our initial status
            let initial_message = format!("[{}] Started monitoring: {}", monitor.get_notes(), change.message);
            
            // Send notification about monitoring start with initial content
            if let Some(notifier) = &notifier {
                if let Err(e) = notifier.send(&initial_message, &change.details).await {
                    let _ = tx.send(Message::Log(
                        format!("Failed to send initial notification: {}", e),
                        Color32::RED
                    )).await;
                } else {
                    let _ = tx.send(Message::Log(
                        format!("Initial notification sent: {}", initial_message),
                        Color32::LIGHT_BLUE
                    )).await;
                }
            }
            
            // Also send to our message system
            let _ = tx.send(Message::ChangeDetected(task_index, change.clone())).await;
        },
        Ok(None) => {
            // Normal case - no change but we have initial content
            // Get task name and use it in initial notification
            let task_note = &monitor.get_name();
            let initial_message = format!("[{}] Started monitoring: {}", monitor.get_notes(), task_note);
            let details = format!("Initial content captured. Will notify when changes are detected.");
            
            // Send notification about monitoring start
            if let Some(notifier) = &notifier {
                if let Err(e) = notifier.send(&initial_message, &details).await {
                    let _ = tx.send(Message::Log(
                        format!("Failed to send initial notification: {}", e),
                        Color32::RED
                    )).await;
                } else {
                    let _ = tx.send(Message::Log(
                        format!("Initial notification sent: {}", initial_message),
                        Color32::LIGHT_BLUE
                    )).await;
                }
            }
            
            // Log the start
            let _ = tx.send(Message::Log(
                format!("Task #{} initialized with initial content", task_index + 1),
                Color32::LIGHT_GREEN
            )).await;
        },
        Err(e) => {
            // Error on first check
            let _ = tx.send(Message::TaskStatusChanged(
                task_index,
                TaskStatus::Error
            )).await;
            
            let _ = tx.send(Message::Log(
                format!("Error getting initial content: {}", e),
                Color32::RED
            )).await;
            
            // Wait for a while before retrying
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        },
    }
    
    // Main monitoring loop
    loop {
        match monitor.check().await {
            Ok(Some(change)) => {
                // Send change detection message
                let _ = tx.send(Message::ChangeDetected(task_index, change.clone())).await;
                
                // Send notification with notes in title
                if let Some(notifier) = &notifier {
                    let notification_title = format!("[{}] {}", monitor.get_notes(), change.message);
                    if let Err(e) = notifier.send(&notification_title, &change.details).await {
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
            Err(_e) => {
                // Send error message
                let _ = tx.send(Message::TaskStatusChanged(
                    task_index,
                    TaskStatus::Error
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
        // Remove incompatible options
        ..Default::default()
    };
    
    eframe::run_native(
        "Hyperliquid Monitoring System",
        native_options,
        Box::new(|cc| Box::new(MonitorApp::new(cc)))
    )
} 