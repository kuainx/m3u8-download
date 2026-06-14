use m3u8_downloader_core::downloader::{DownloadTask, TaskStatus};
use m3u8_downloader_core::AppConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::state::{BatchTaskItem, BatchTaskStatus, M3u8App};
use super::utils::{
    ensure_mp4_filename, extract_filename_from_url, find_existing_matching_file, parse_task_line,
    progress_detail,
};

impl M3u8App {
    pub(super) fn start_queue(&mut self) {
        let tasks = self.parse_batch_tasks();
        if tasks.is_empty() {
            self.add_log("❌ 请输入至少一个下载任务".into(), true);
            return;
        }

        self.batch_tasks = tasks;
        self.active_task = None;
        self.active_task_index = None;
        self.progress_tracker = None;
        self.last_progress = None;
        self.log_receiver = None;
        self.queue_running = true;

        self.add_log(
            format!("📦 已加入 {} 个任务", self.batch_tasks.len()),
            false,
        );
        self.start_next_task();
    }

    pub(super) fn start_next_task(&mut self) {
        while self.queue_running {
            let Some(task_index) = self
                .batch_tasks
                .iter()
                .position(|task| task.status == BatchTaskStatus::Pending)
            else {
                self.queue_running = false;
                self.active_task = None;
                self.active_task_index = None;
                self.progress_tracker = None;
                self.last_progress = None;
                self.log_receiver = None;
                self.add_log("✅ 批量任务执行完成".into(), false);
                if let Some(ref tp) = self.taskbar_progress {
                    tp.clear();
                }
                return;
            };

            let batch_task = self.batch_tasks[task_index].clone();
            if let Some(existing_name) =
                find_existing_matching_file(&self.save_path, &batch_task.output_filename)
            {
                self.batch_tasks[task_index].status = BatchTaskStatus::Skipped;
                self.batch_tasks[task_index].detail =
                    format!("检测到已存在文件: {}", existing_name);
                self.add_log(
                    format!(
                        "⏭ 跳过 {}: 检测到已存在匹配文件 {}",
                        batch_task.output_filename, existing_name
                    ),
                    false,
                );
                continue;
            }

            let mut config = AppConfig::default();
            config.save_path = PathBuf::from(&self.save_path);
            config.concurrent_downloads = self.concurrent_downloads;
            config.temp_name_strategy = self.temp_name_strategy;
            config.temp_dir = config.save_path.join("./");

            let task_arc = Arc::new(DownloadTask::new(
                batch_task.url.clone(),
                config,
                batch_task.output_filename.clone(),
            ));
            let progress_arc = task_arc.progress.clone();

            let (tx, rx) = mpsc::unbounded_channel();
            self.log_receiver = Some(rx);

            self.active_task = Some(task_arc.clone());
            self.active_task_index = Some(task_index);
            self.progress_tracker = Some(progress_arc);
            self.last_progress = None;

            self.batch_tasks[task_index].status = BatchTaskStatus::Running;
            self.batch_tasks[task_index].detail = "等待开始".to_string();

            self.add_log(
                format!("▶ 开始下载: {}", &batch_task.output_filename),
                false,
            );

            self.runtime.spawn(async move {
                task_arc.set_log_sender(tx).await;
                let res = task_arc.run().await;

                match res {
                    Ok(path) => {
                        eprintln!("✅ 下载完成: {}", path.display());
                    }
                    Err(e) => {
                        eprintln!("❌ 下载失败: {}", e);
                    }
                }
            });
            return;
        }
    }

    pub fn stop_queue(&mut self) {
        self.queue_running = false;
        if let Some(ref task) = self.active_task {
            task.cancel();
            self.add_log("⏹ 已停止队列并取消当前任务".into(), false);
        } else {
            self.add_log("⏹ 已停止队列".into(), false);
        }
    }

    pub(super) fn handle_ffmpeg_logs(&mut self) {
        if let Some(mut rx) = self.log_receiver.take() {
            while let Ok(msg) = rx.try_recv() {
                self.add_log(msg, false);
            }
            self.log_receiver = Some(rx);
        }
    }

    pub fn poll_progress(&mut self) {
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
            if let Some(task_index) = self.active_task_index {
                self.batch_tasks[task_index].detail = progress_detail(&progress);
            }

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
                        if let Some(task_index) = self.active_task_index {
                            self.batch_tasks[task_index].status = BatchTaskStatus::Completed;
                            self.batch_tasks[task_index].detail = format!("已完成: {}", path);
                        }
                    }
                    TaskStatus::Failed(e) => {
                        self.add_log(format!("❌ 下载失败: {}", e), true);
                        if let Some(task_index) = self.active_task_index {
                            self.batch_tasks[task_index].status = BatchTaskStatus::Failed;
                            self.batch_tasks[task_index].detail = e.clone();
                        }
                    }
                    TaskStatus::Cancelled => {
                        self.add_log("⏹ 任务已取消".into(), false);
                        if let Some(task_index) = self.active_task_index {
                            self.batch_tasks[task_index].status = BatchTaskStatus::Cancelled;
                            self.batch_tasks[task_index].detail = "任务已取消".to_string();
                        }
                    }
                    _ => {}
                }
            }
            self.last_progress = Some(progress);
        }

        self.update_taskbar();

        if matches!(
            self.last_progress.as_ref().map(|p| &p.status),
            Some(TaskStatus::Completed) | Some(TaskStatus::Failed(_)) | Some(TaskStatus::Cancelled)
        ) {
            self.finish_active_task();
        }
    }

    pub fn finish_active_task(&mut self) {
        self.active_task = None;
        self.active_task_index = None;
        self.progress_tracker = None;
        self.log_receiver = None;
        self.last_progress = None;

        if let Some(ref tp) = self.taskbar_progress {
            tp.clear();
        }

        if self.queue_running {
            self.start_next_task();
        }
    }

    pub(super) fn parse_batch_tasks(&self) -> Vec<BatchTaskItem> {
        let lines: Vec<&str> = self
            .task_input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let is_single_task = lines.len() == 1;

        lines
            .into_iter()
            .filter_map(|line| {
                let (url, explicit_filename) = parse_task_line(line);
                if url.trim().is_empty() {
                    return None;
                }

                let filename = if let Some(filename) = explicit_filename {
                    filename
                } else if is_single_task && !self.output_filename.trim().is_empty() {
                    self.output_filename.trim().to_string()
                } else {
                    extract_filename_from_url(&url).unwrap_or_else(|| "output.mp4".to_string())
                };

                Some(BatchTaskItem {
                    url,
                    output_filename: ensure_mp4_filename(&filename),
                    status: BatchTaskStatus::Pending,
                    detail: "等待执行".to_string(),
                })
            })
            .collect()
    }

    pub(super) fn update_output_filename_from_input(&mut self) {
        let mut lines = self
            .task_input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let Some(line) = lines.next() else {
            return;
        };

        if lines.next().is_some() {
            return;
        }

        let (url, explicit_filename) = parse_task_line(line);
        if explicit_filename.is_some() {
            return;
        }

        if let Some(filename) = extract_filename_from_url(&url) {
            self.output_filename = filename;
        }
    }

    pub(super) fn update_taskbar(&mut self) {
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
