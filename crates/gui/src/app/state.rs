use super::taskbar::TaskbarProgress;
use m3u8_downloader_core::downloader::{DownloadProgress, DownloadTask};
use m3u8_downloader_core::{AppConfig, TempNameStrategy};
use std::sync::Arc;
use tokio::sync::mpsc;

pub(super) const CONFIG_FILE: &str = "m3u8_dl.cfg";

#[derive(Default)]
pub(super) struct GuiConfig {
    pub save_path: Option<String>,
    pub concurrent_downloads: Option<usize>,
    pub temp_name_strategy: Option<TempNameStrategy>,
}

impl GuiConfig {
    pub(super) fn load() -> Self {
        let mut config = GuiConfig::default();
        if let Ok(content) = std::fs::read_to_string(CONFIG_FILE) {
            for line in content.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "save_path" => config.save_path = Some(v.trim().to_string()),
                        "concurrent_downloads" => {
                            config.concurrent_downloads = v.trim().parse().ok()
                        }
                        "temp_name_strategy" => {
                            config.temp_name_strategy = match v.trim() {
                                "ContentHash" => Some(TempNameStrategy::ContentHash),
                                _ => Some(TempNameStrategy::Filename),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        config
    }

    pub(super) fn save(save_path: &str, concurrent_downloads: usize, strategy: TempNameStrategy) {
        let strategy_str = match strategy {
            TempNameStrategy::ContentHash => "ContentHash",
            TempNameStrategy::Filename => "Filename",
        };
        let content = format!(
            "save_path={}\nconcurrent_downloads={}\ntemp_name_strategy={}\n",
            save_path, concurrent_downloads, strategy_str
        );
        let _ = std::fs::write(CONFIG_FILE, content);
    }
}

pub(super) struct LogEntry {
    pub timestamp: String,
    pub message: String,
    pub is_error: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchTaskStatus {
    Pending,
    Running,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

impl BatchTaskStatus {
    pub(super) fn label(&self) -> &'static str {
        match self {
            BatchTaskStatus::Pending => "待执行",
            BatchTaskStatus::Running => "下载中",
            BatchTaskStatus::Completed => "已完成",
            BatchTaskStatus::Skipped => "已跳过",
            BatchTaskStatus::Failed => "失败",
            BatchTaskStatus::Cancelled => "已取消",
        }
    }
}

#[derive(Clone)]
pub(super) struct BatchTaskItem {
    pub url: String,
    pub output_filename: String,
    pub status: BatchTaskStatus,
    pub detail: String,
}

pub struct M3u8App {
    pub(super) task_input: String,
    pub(super) output_filename: String,
    pub(super) save_path: String,
    pub(super) concurrent_downloads: usize,
    pub(super) temp_name_strategy: TempNameStrategy,
    pub(super) runtime: tokio::runtime::Runtime,
    pub(super) batch_tasks: Vec<BatchTaskItem>,
    pub(super) active_task: Option<Arc<DownloadTask>>,
    pub(super) active_task_index: Option<usize>,
    pub(super) progress_tracker: Option<Arc<std::sync::Mutex<DownloadProgress>>>,
    pub(super) last_progress: Option<DownloadProgress>,
    pub(super) queue_running: bool,
    pub(super) logs: Vec<LogEntry>,
    pub(super) log_receiver: Option<mpsc::UnboundedReceiver<String>>,
    pub(super) taskbar_progress: Option<TaskbarProgress>,
}

impl M3u8App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        super::theme::configure_fonts(&cc.egui_ctx);
        super::theme::configure_style(&cc.egui_ctx);

        let config = AppConfig::default();
        let mut save_path = config.save_path.to_string_lossy().to_string();
        let mut concurrent = config.concurrent_downloads;
        let mut strategy = config.temp_name_strategy;

        let gui_config = GuiConfig::load();
        if let Some(p) = gui_config.save_path {
            save_path = p;
        }
        if let Some(c) = gui_config.concurrent_downloads {
            concurrent = c;
        }
        if let Some(s) = gui_config.temp_name_strategy {
            strategy = s;
        }

        Self {
            task_input: String::new(),
            output_filename: "output.mp4".to_string(),
            save_path,
            concurrent_downloads: concurrent,
            temp_name_strategy: strategy,
            runtime: tokio::runtime::Runtime::new().unwrap(),
            batch_tasks: Vec::new(),
            active_task: None,
            active_task_index: None,
            progress_tracker: None,
            last_progress: None,
            queue_running: false,
            logs: Vec::new(),
            log_receiver: None,
            taskbar_progress: None,
        }
    }

    pub(super) fn add_log(&mut self, message: String, is_error: bool) {
        let now = super::utils::chrono_now();
        self.logs.push(LogEntry {
            timestamp: now,
            message,
            is_error,
        });
        if self.logs.len() > 500 {
            self.logs.drain(0..self.logs.len() - 500);
        }
    }
}
