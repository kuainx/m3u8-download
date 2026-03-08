use eframe::egui;
use m3u8_downloader_core::downloader::{DownloadProgress, DownloadTask, TaskStatus};
use m3u8_downloader_core::{AppConfig, TempNameStrategy};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    ITaskbarList3, TaskbarList, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL,
};

const CONFIG_FILE: &str = "m3u8_dl.cfg";

#[derive(Default)]
struct GuiConfig {
    pub save_path: Option<String>,
    pub concurrent_downloads: Option<usize>,
    pub temp_name_strategy: Option<TempNameStrategy>,
}

impl GuiConfig {
    fn load() -> Self {
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

    fn save(save_path: &str, concurrent_downloads: usize, strategy: TempNameStrategy) {
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

/// 日志条目
struct LogEntry {
    timestamp: String,
    message: String,
    is_error: bool,
}

/// 封装任务栏进度管理
struct TaskbarProgress {
    taskbar: Option<ITaskbarList3>,
    hwnd: HWND,
}

impl TaskbarProgress {
    fn new(hwnd: HWND) -> Self {
        let taskbar = unsafe {
            windows::Win32::System::Com::CoCreateInstance(
                &TaskbarList,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .ok()
        };
        Self { taskbar, hwnd }
    }

    fn set_progress(&self, completed: u64, total: u64) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_NORMAL);
                let _ = taskbar.SetProgressValue(self.hwnd, completed, total);
            }
        }
    }

    fn set_indeterminate(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_INDETERMINATE);
            }
        }
    }

    fn set_error(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_ERROR);
            }
        }
    }

    fn clear(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_NOPROGRESS);
            }
        }
    }
}

/// 主应用状态
pub struct M3u8App {
    // 输入
    url_input: String,
    output_filename: String,
    save_path: String,

    // 配置
    concurrent_downloads: usize,
    temp_name_strategy: TempNameStrategy,

    // 运行时
    runtime: tokio::runtime::Runtime,

    // 任务状态
    active_task: Option<Arc<DownloadTask>>,
    progress_tracker: Option<Arc<std::sync::Mutex<DownloadProgress>>>,
    last_progress: Option<DownloadProgress>,

    // 日志
    logs: Vec<LogEntry>,
    log_receiver: Option<mpsc::UnboundedReceiver<String>>,

    // 平台相关
    taskbar_progress: Option<TaskbarProgress>,
}

impl M3u8App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 加载中文字体
        configure_fonts(&cc.egui_ctx);
        // 设置深色主题 + 现代化样式
        configure_style(&cc.egui_ctx);

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
            url_input: String::new(),
            output_filename: "output.mp4".to_string(),
            save_path,
            concurrent_downloads: concurrent,
            temp_name_strategy: strategy,
            runtime: tokio::runtime::Runtime::new().unwrap(),
            active_task: None,
            progress_tracker: None,
            last_progress: None,
            logs: Vec::new(),
            log_receiver: None,
            taskbar_progress: None,
        }
    }

    fn add_log(&mut self, message: String, is_error: bool) {
        let now = chrono_now();
        self.logs.push(LogEntry {
            timestamp: now,
            message,
            is_error,
        });
        // 保留最近 500 条日志
        if self.logs.len() > 500 {
            self.logs.drain(0..self.logs.len() - 500);
        }
    }

    fn start_download(&mut self) {
        if self.url_input.trim().is_empty() {
            self.add_log("❌ 请输入 M3U8 URL".into(), true);
            return;
        }

        let mut config = AppConfig::default();
        config.save_path = PathBuf::from(&self.save_path);
        config.concurrent_downloads = self.concurrent_downloads;
        config.temp_name_strategy = self.temp_name_strategy;
        config.temp_dir = config.save_path.join("./");

        let url = self.url_input.trim().to_string();
        let filename = if self.output_filename.trim().is_empty() {
            "output.mp4".to_string()
        } else {
            self.output_filename.trim().to_string()
        };

        let task_arc = Arc::new(DownloadTask::new(url.clone(), config, filename));
        let progress_arc = task_arc.progress.clone();

        let (tx, rx) = mpsc::unbounded_channel();
        self.log_receiver = Some(rx);

        self.active_task = Some(task_arc.clone());
        self.progress_tracker = Some(progress_arc);

        self.add_log(format!("▶ 开始下载: {}", &url), false);

        // 在后台运行下载任务
        self.runtime.spawn(async move {
            task_arc.set_log_sender(tx).await;
            let res = task_arc.run().await;

            match res {
                Ok(path) => {
                    eprintln!("✅ 下载完成: {}", path.display());
                }
                Err(e) => {
                    eprintln!("❌ 下载失败: {}", e);
                    // 注意：这里我们不再需要锁定整个 task 来记录失败，
                    // 因为 DownloadTask::run 内部已经在出错时尝试处理状态了。
                    // 如果 run 因重试用尽或其他原因退出，我们可以通过进度对象更新状态。
                }
            }
        });
    }

    fn cancel_download(&mut self) {
        if let Some(ref task) = self.active_task {
            task.cancel();
            self.add_log("⏹ 已取消下载任务".into(), false);
        }
    }

    fn handle_ffmpeg_logs(&mut self) {
        if let Some(mut rx) = self.log_receiver.take() {
            while let Ok(msg) = rx.try_recv() {
                self.add_log(msg, false);
            }
            self.log_receiver = Some(rx);
        }
    }

    fn poll_progress(&mut self) {
        // 处理 ffmpeg 日志
        self.handle_ffmpeg_logs();

        let progress = if let Some(ref tracker) = self.progress_tracker {
            if let Ok(prog_guard) = tracker.try_lock() {
                Some(prog_guard.clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(progress) = progress {
            // 状态变化时写日志
            let status_changed = match (&self.last_progress, &progress.status) {
                (Some(old), new) => {
                    std::mem::discriminant(&old.status) != std::mem::discriminant(new)
                }
                (None, _) => true,
            };
            if status_changed {
                match &progress.status {
                    TaskStatus::Parsing => {
                        self.add_log("🔍 正在解析 M3U8 播放列表...".into(), false)
                    }
                    TaskStatus::Downloading { total, .. } => {
                        self.add_log(format!("⬇ 开始下载 {} 个分片...", total), false)
                    }
                    TaskStatus::Merging => self.add_log("🔗 正在合并分片...".into(), false),
                    TaskStatus::Completed => {
                        let path = progress
                            .output_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        self.add_log(format!("✅ 下载完成: {}", path), false);
                    }
                    TaskStatus::Failed(e) => self.add_log(format!("❌ 下载失败: {}", e), true),
                    TaskStatus::Cancelled => self.add_log("⏹ 任务已取消".into(), false),
                    _ => {}
                }
            }
            self.last_progress = Some(progress);
        }

        // 更新任务栏进度
        self.update_taskbar();
    }

    fn update_taskbar(&mut self) {
        if let Some(ref progress) = self.last_progress {
            if let Some(ref tp) = self.taskbar_progress {
                match &progress.status {
                    TaskStatus::Parsing | TaskStatus::Merging => tp.set_indeterminate(),
                    TaskStatus::Downloading { completed, total } => {
                        tp.set_progress(*completed as u64, *total as u64)
                    }
                    TaskStatus::Failed(_) => tp.set_error(),
                    TaskStatus::Completed | TaskStatus::Cancelled => tp.clear(),
                    _ => tp.clear(),
                }
            }
        }
    }
}

impl eframe::App for M3u8App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 初始化任务栏对象（仅一次，需要窗口句柄）
        if self.taskbar_progress.is_none() {
            if let Ok(hwnd) = frame.window_handle() {
                if let RawWindowHandle::Win32(handle) = hwnd.as_raw() {
                    self.taskbar_progress =
                        Some(TaskbarProgress::new(HWND(handle.hwnd.get() as *mut _)));
                }
            }
        }

        // 轮询进度
        self.poll_progress();

        // 持续刷新（下载中）
        if let Some(ref prog) = self.last_progress {
            match prog.status {
                TaskStatus::Downloading { .. } | TaskStatus::Parsing | TaskStatus::Merging => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                _ => {}
            }
        }

        // ========== 顶部标题栏 ==========
        egui::TopBottomPanel::top("title_bar").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.heading(
                    egui::RichText::new("⬇ M3U8 Downloader")
                        .size(22.0)
                        .strong()
                        .color(egui::Color32::from_rgb(100, 180, 255)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("v0.1.0")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(128, 128, 140)),
                    );
                });
            });
            ui.add_space(6.0);
        });

        // ========== 底部日志面板 ==========
        egui::TopBottomPanel::bottom("log_panel")
            .min_height(140.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("📋 日志")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(180, 180, 195)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(
                                egui::RichText::new("清空")
                                    .color(egui::Color32::from_rgb(160, 160, 175)),
                            )
                            .clicked()
                        {
                            self.logs.clear();
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for entry in &self.logs {
                            let color = if entry.is_error {
                                egui::Color32::from_rgb(255, 100, 100)
                            } else {
                                egui::Color32::from_rgb(170, 170, 185)
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&entry.timestamp)
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(100, 100, 115))
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(&entry.message).size(12.5).color(color),
                                );
                            });
                        }
                    });
            });

        // ========== 中央面板 ==========
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);

            // --- URL 输入 ---
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("🔗 M3U8 URL")
                        .size(14.0)
                        .color(egui::Color32::from_rgb(180, 180, 195)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let text_edit = egui::TextEdit::singleline(&mut self.url_input)
                    .hint_text("请输入 M3U8 播放列表 URL...")
                    .desired_width(ui.available_width() - 24.0)
                    .font(egui::TextStyle::Body)
                    .margin(egui::Margin::symmetric(10, 8));
                let response = ui.add(text_edit);
                if response.changed() {
                    if let Some(filename) = extract_filename_from_url(&self.url_input) {
                        self.output_filename = filename;
                    }
                }
                ui.add_space(12.0);
            });

            ui.add_space(12.0);

            // --- 文件名 + 保存路径 ---
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                // 输出文件名
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("📄 输出文件名")
                            .size(13.0)
                            .color(egui::Color32::from_rgb(180, 180, 195)),
                    );
                    ui.add_space(2.0);
                    let name_edit = egui::TextEdit::singleline(&mut self.output_filename)
                        .hint_text("output.ts")
                        .desired_width(220.0)
                        .margin(egui::Margin::symmetric(10, 8));
                    ui.add(name_edit);
                });

                ui.add_space(16.0);

                // 保存路径
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("📁 保存路径")
                            .size(13.0)
                            .color(egui::Color32::from_rgb(180, 180, 195)),
                    );
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let path_edit = egui::TextEdit::singleline(&mut self.save_path)
                            .desired_width(ui.available_width() - 125.0)
                            .margin(egui::Margin::symmetric(10, 8));
                        if ui.add(path_edit).changed() {
                            GuiConfig::save(
                                &self.save_path,
                                self.concurrent_downloads,
                                self.temp_name_strategy,
                            );
                        }
                        if ui.button(egui::RichText::new("浏览").size(13.0)).clicked() {
                            if let Some(folder) = rfd::FileDialog::new()
                                .set_directory(&self.save_path)
                                .pick_folder()
                            {
                                self.save_path = folder.display().to_string();
                                GuiConfig::save(
                                    &self.save_path,
                                    self.concurrent_downloads,
                                    self.temp_name_strategy,
                                );
                            }
                        }
                        if ui.button(egui::RichText::new("打开").size(13.0)).clicked() {
                            let path = std::path::Path::new(&self.save_path);
                            let final_path = if path.is_absolute() {
                                path.to_path_buf()
                            } else {
                                std::env::current_dir().unwrap_or_default().join(path)
                            };

                            let _ = std::process::Command::new("explorer")
                                .arg(final_path)
                                .spawn();
                        }
                    });
                });

                ui.add_space(12.0);
            });

            ui.add_space(12.0);

            // --- 并发数设置 ---
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("⚡ 并发下载数")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(180, 180, 195)),
                );
                ui.add_space(8.0);
                let mut concurrent = self.concurrent_downloads as f32;
                let slider = egui::Slider::new(&mut concurrent, 1.0..=64.0)
                    .step_by(1.0)
                    .text("线程");
                if ui.add(slider).changed() {
                    self.concurrent_downloads = concurrent as usize;
                    GuiConfig::save(
                        &self.save_path,
                        self.concurrent_downloads,
                        self.temp_name_strategy,
                    );
                }
                self.concurrent_downloads = concurrent as usize;
                ui.add_space(12.0);
            });

            ui.add_space(12.0);

            // --- 临时文件名设置 ---
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("📂 临时文件名")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(180, 180, 195)),
                );
                ui.add_space(8.0);
                let res1 = ui.radio_value(
                    &mut self.temp_name_strategy,
                    TempNameStrategy::Filename,
                    "m3u8文件名",
                );
                let res2 = ui.radio_value(
                    &mut self.temp_name_strategy,
                    TempNameStrategy::ContentHash,
                    "m3u8内容哈希",
                );
                if res1.changed() || res2.changed() {
                    GuiConfig::save(
                        &self.save_path,
                        self.concurrent_downloads,
                        self.temp_name_strategy,
                    );
                }
                ui.add_space(12.0);
            });

            ui.add_space(16.0);

            // --- 控制按钮 ---
            ui.horizontal(|ui| {
                ui.add_space(12.0);

                let is_running = matches!(
                    self.last_progress.as_ref().map(|p| &p.status),
                    Some(TaskStatus::Downloading { .. })
                        | Some(TaskStatus::Parsing)
                        | Some(TaskStatus::Merging)
                );

                if !is_running {
                    let download_btn = egui::Button::new(
                        egui::RichText::new("⬇  开始下载")
                            .size(16.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(50, 120, 220))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(160.0, 42.0));

                    if ui.add(download_btn).clicked() {
                        self.start_download();
                    }
                } else {
                    let cancel_btn = egui::Button::new(
                        egui::RichText::new("⏹  取消下载")
                            .size(16.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(200, 60, 60))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(160.0, 42.0));

                    if ui.add(cancel_btn).clicked() {
                        self.cancel_download();
                    }
                }

                ui.add_space(12.0);
            });

            ui.add_space(16.0);

            // --- 进度显示 ---
            if let Some(ref progress) = self.last_progress {
                ui.add_space(6.0);
                render_progress(ui, progress);
            }
        });
    }
}

/// 渲染进度信息
fn render_progress(ui: &mut egui::Ui, progress: &DownloadProgress) {
    ui.vertical(|ui| {
        ui.add_space(4.0);
        match &progress.status {
            TaskStatus::Pending => {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("⏳ 等待中...")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(180, 180, 195)),
                    );
                });
            }
            TaskStatus::Parsing => {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("🔍 解析 M3U8 播放列表中...")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(100, 200, 255)),
                    );
                    ui.add(egui::Spinner::new().size(20.0));
                });
            }
            TaskStatus::Downloading { completed, total } => {
                let pct = if *total > 0 {
                    *completed as f32 / *total as f32
                } else {
                    0.0
                };

                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "⬇ 下载中: {}/{} 分片 ({:.1}%)",
                            completed,
                            total,
                            pct * 100.0
                        ))
                        .size(14.0)
                        .color(egui::Color32::from_rgb(100, 220, 150)),
                    );

                    let mut extras = Vec::new();
                    if let Some(size) = progress.estimated_total_size {
                        extras.push(format!("视频大小: {}", format_bytes(size)));
                    }
                    if let Some(duration) = progress.estimated_total_duration {
                        extras.push(format!("视频时长: {}", format_duration(duration)));
                    }
                    if let Some(eta) = progress.eta_seconds {
                        extras.push(format!("剩余时间: {}", format_duration(eta as f64)));
                    }

                    if !extras.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(18.0);
                            ui.label(
                                egui::RichText::new(extras.join("  "))
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(140, 140, 160)),
                            );
                        });
                    }
                });

                ui.add_space(4.0);

                // 自定义进度条
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let available = ui.available_width() - 18.0;
                    let progress_bar = egui::ProgressBar::new(pct)
                        .show_percentage()
                        .animate(true)
                        .desired_width(available);
                    ui.add(progress_bar);
                });
            }
            TaskStatus::Merging => {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("🔗 合并分片中...")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(255, 200, 100)),
                    );
                    ui.add(egui::Spinner::new().size(20.0));
                });
            }
            TaskStatus::Completed => {
                let path_str = progress
                    .output_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("✅ 下载完成: {}", path_str))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(80, 220, 120)),
                    );
                });
            }
            TaskStatus::Failed(err) => {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("❌ 失败: {}", err))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(255, 90, 90)),
                    );
                });
            }
            TaskStatus::Cancelled => {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("⏹ 任务已取消")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(200, 200, 210)),
                    );
                });
            }
        }
    });
}

/// 配置中文字体
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载系统中文字体（微软雅黑）
    let font_path = std::path::Path::new("C:\\Windows\\Fonts\\msyh.ttc");
    if font_path.exists() {
        if let Ok(font_data) = std::fs::read(font_path) {
            fonts.font_data.insert(
                "msyh".to_owned(),
                egui::FontData::from_owned(font_data).into(),
            );

            // 将中文字体插入到 Proportional 和 Monospace 的首位
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "msyh".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("msyh".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

/// 配置深色主题现代化样式
fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // 深色调色板
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::from_rgb(22, 22, 30);
    visuals.panel_fill = egui::Color32::from_rgb(22, 22, 30);
    visuals.faint_bg_color = egui::Color32::from_rgb(32, 32, 42);
    visuals.extreme_bg_color = egui::Color32::from_rgb(16, 16, 22);

    // 圆角
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);

    // 输入框背景
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 48);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 42, 58);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(50, 50, 68);

    // 高亮色
    visuals.selection.bg_fill = egui::Color32::from_rgb(50, 100, 200);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 160, 255));

    style.visuals = visuals;

    // 间距调整
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);

    ctx.set_style(style);
}

/// 获取当前时间字符串
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

/// 提取 URL 中的文件名
fn extract_filename_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // 移除 query string 和 fragment
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);

    // 获取最后一段作为文件名
    let last_segment = path.split('/').last().unwrap_or("");
    if last_segment.is_empty() {
        return None;
    }

    // 如果是 .m3u8 结尾，则替换为 .mp4
    let mut filename = last_segment.to_string();
    if filename.ends_with(".m3u8") {
        filename = filename.replace(".m3u8", ".mp4");
    }
    Some(filename)
}

/// 格式化字节数为人类可读字符串
fn format_bytes(bytes: u64) -> String {
    let mut size = bytes as f64;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < units.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", size as u64, units[unit_idx])
    } else {
        format!("{:.2} {}", size, units[unit_idx])
    }
}

/// 格式化秒数为 HH:MM:SS
fn format_duration(seconds: f64) -> String {
    let secs = seconds as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}
