use super::state::{BatchTaskItem, BatchTaskStatus};
use super::utils::{format_bitrate, format_bytes, format_duration};
use eframe::egui;
use m3u8_downloader_core::downloader::{DownloadProgress, TaskStatus};

pub(super) fn render_progress(ui: &mut egui::Ui, progress: &DownloadProgress) {
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
                        if let Some(size) = progress.estimated_total_size {
                            extras.push(format!("比特率: {}", format_bitrate(size, duration)));
                        }
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

pub(super) fn render_task_summary(ui: &mut egui::Ui, tasks: &[BatchTaskItem]) {
    let completed = tasks
        .iter()
        .filter(|task| task.status == BatchTaskStatus::Completed)
        .count();
    let skipped = tasks
        .iter()
        .filter(|task| task.status == BatchTaskStatus::Skipped)
        .count();
    let failed = tasks
        .iter()
        .filter(|task| task.status == BatchTaskStatus::Failed)
        .count();
    let remaining = tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                BatchTaskStatus::Pending | BatchTaskStatus::Running
            )
        })
        .count();

    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!(
                "任务总数: {}  剩余: {}  已完成: {}  已跳过: {}  失败: {}",
                tasks.len(),
                remaining,
                completed,
                skipped,
                failed
            ))
            .size(13.0)
            .color(egui::Color32::from_rgb(160, 160, 175)),
        );
    });
}

pub(super) fn render_task_list(ui: &mut egui::Ui, tasks: &[BatchTaskItem]) {
    egui::ScrollArea::vertical()
        .max_height(180.0)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for (index, task) in tasks.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "#{:02} [{}] {}",
                            index + 1,
                            task.status.label(),
                            task.output_filename
                        ))
                        .size(13.0)
                        .color(egui::Color32::from_rgb(190, 190, 205)),
                    );
                    ui.label(
                        egui::RichText::new(format!(" - {}", task.detail))
                            .size(12.0)
                            .color(egui::Color32::from_rgb(140, 140, 160)),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(28.0);
                    ui.label(
                        egui::RichText::new(&task.url)
                            .size(11.5)
                            .color(egui::Color32::from_rgb(110, 110, 125)),
                    );
                });
                ui.add_space(6.0);
            }
        });
}
