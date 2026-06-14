mod controller;
mod state;
mod taskbar;
mod theme;
mod utils;
mod view;
mod widgets;

use eframe::egui;
use m3u8_downloader_core::downloader::TaskStatus;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

pub use state::M3u8App;
use taskbar::TaskbarProgress;

impl eframe::App for M3u8App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.taskbar_progress.is_none() {
            if let Ok(hwnd) = frame.window_handle() {
                if let RawWindowHandle::Win32(handle) = hwnd.as_raw() {
                    self.taskbar_progress =
                        Some(TaskbarProgress::new(HWND(handle.hwnd.get() as *mut _)));
                }
            }
        }

        self.poll_progress();

        if self.queue_running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if let Some(ref prog) = self.last_progress {
            match prog.status {
                TaskStatus::Downloading { .. } | TaskStatus::Parsing | TaskStatus::Merging => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                _ => {}
            }
        }

        self.render_title_bar(ctx);
        self.render_log_panel(ctx);
        self.render_main_panel(ctx);
    }
}
