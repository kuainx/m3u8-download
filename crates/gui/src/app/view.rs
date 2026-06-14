use eframe::egui;

use super::state::{GuiConfig, M3u8App};
use super::widgets::{render_progress, render_task_list, render_task_summary};

impl M3u8App {
    pub(super) fn render_title_bar(&self, ctx: &egui::Context) {
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
                        egui::RichText::new("v0.2.0")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(128, 128, 140)),
                    );
                });
            });
            ui.add_space(6.0);
        });
    }

    pub(super) fn render_log_panel(&mut self, ctx: &egui::Context) {
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
    }

    pub(super) fn render_main_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);

            // 批量任务输入
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("🔗 批量任务")
                        .size(14.0)
                        .color(egui::Color32::from_rgb(180, 180, 195)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let available_width = ui.available_width() - 24.0;
                let mut response = None;
                ui.allocate_ui_with_layout(
                    egui::vec2(available_width, 120.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let input_bg = ui.visuals().widgets.inactive.bg_fill;
                        let input_stroke = ui.visuals().widgets.inactive.bg_stroke;
                        egui::Frame::default()
                            .fill(input_bg)
                            .stroke(input_stroke)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(2, 2))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(112.0)
                                    .auto_shrink([false; 2])
                                    .show(ui, |ui| {
                                        let text_edit =
                                            egui::TextEdit::multiline(&mut self.task_input)
                                                .hint_text("每行一个任务，支持 URL 或 URL|文件名")
                                                .desired_rows(5)
                                                .desired_width(ui.available_width())
                                                .frame(false)
                                                .font(egui::TextStyle::Body)
                                                .margin(egui::Margin::symmetric(10, 6));
                                        response = Some(ui.add(text_edit));
                                    });
                            });
                    },
                );
                if response.as_ref().is_some_and(|resp| resp.changed()) {
                    self.update_output_filename_from_input();
                }
                ui.add_space(12.0);
            });

            ui.add_space(12.0);

            // 文件名 + 保存路径
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
                        .hint_text("单条任务时生效")
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

            // 并发数设置
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

            // 临时文件名设置
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
                    m3u8_downloader_core::TempNameStrategy::Filename,
                    "m3u8文件名",
                );
                let res2 = ui.radio_value(
                    &mut self.temp_name_strategy,
                    m3u8_downloader_core::TempNameStrategy::ContentHash,
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

            // 控制按钮
            ui.horizontal(|ui| {
                ui.add_space(12.0);

                let is_running = self.queue_running || self.active_task.is_some();

                if !is_running {
                    let download_btn = egui::Button::new(
                        egui::RichText::new("⬇  开始队列")
                            .size(16.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(50, 120, 220))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(160.0, 42.0));

                    if ui.add(download_btn).clicked() {
                        self.start_queue();
                    }
                } else {
                    let cancel_btn = egui::Button::new(
                        egui::RichText::new("⏹  停止队列")
                            .size(16.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(200, 60, 60))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(160.0, 42.0));

                    if ui.add(cancel_btn).clicked() {
                        self.stop_queue();
                    }
                }

                ui.add_space(12.0);
            });

            ui.add_space(16.0);

            // 进度显示
            if let Some(ref progress) = self.last_progress {
                ui.add_space(6.0);
                render_progress(ui, progress);
                ui.add_space(12.0);
            }

            // 任务列表
            if !self.batch_tasks.is_empty() {
                render_task_summary(ui, &self.batch_tasks);
                ui.add_space(8.0);
                render_task_list(ui, &self.batch_tasks);
                ui.add_space(12.0);
            }
        });
    }
}
