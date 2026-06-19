use m3u8_downloader_core::downloader::{DownloadProgress, TaskStatus};
use std::path::Path;

pub(super) fn chrono_now() -> String {
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

pub(super) fn extract_filename_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);

    let last_segment = path.split('/').last().unwrap_or("");
    if last_segment.is_empty() {
        return None;
    }

    let mut filename = last_segment.to_string();
    if filename.ends_with(".m3u8") {
        filename = filename.replace(".m3u8", ".mp4");
    }
    Some(filename)
}

pub(super) fn parse_task_line(line: &str) -> (String, Option<String>) {
    if let Some((url, filename)) = line.split_once('|') {
        let url = url.trim().to_string();
        let filename = filename.trim().to_string();
        return if filename.is_empty() {
            (url, None)
        } else {
            (url, Some(filename))
        };
    }

    if let Some((url, filename)) = line.split_once(',') {
        let url = url.trim().to_string();
        let filename = filename.trim().to_string();
        return if filename.is_empty() {
            (url, None)
        } else {
            (url, Some(filename))
        };
    }

    (line.trim().to_string(), None)
}

pub(super) fn ensure_mp4_filename(filename: &str) -> String {
    let filename = filename.trim();
    if filename.to_ascii_lowercase().ends_with(".mp4") {
        filename.to_string()
    } else {
        format!("{filename}.mp4")
    }
}

fn filename_match_keyword(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(filename)
        .trim()
        .to_ascii_lowercase()
}

pub(super) fn find_existing_matching_file(
    save_path: &str,
    target_filename: &str,
) -> Option<String> {
    let keyword = filename_match_keyword(target_filename);
    if keyword.is_empty() {
        return None;
    }

    let save_dir = {
        let path = Path::new(save_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(path)
        }
    };

    let entries = std::fs::read_dir(save_dir).ok()?;
    for entry in entries.flatten() {
        // 只处理文件，跳过目录及其他非文件类型
        if let Ok(metadata) = entry.metadata() {
            if !metadata.is_file() {
                continue;
            }
        } else {
            continue; // 无法获取元数据则跳过
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().contains(&keyword) {
            return Some(name);
        }
    }

    None
}

pub(super) fn progress_detail(progress: &DownloadProgress) -> String {
    match &progress.status {
        TaskStatus::Pending => "等待开始".to_string(),
        TaskStatus::Parsing => "解析中".to_string(),
        TaskStatus::Downloading { completed, total } => format!("下载中 {}/{}", completed, total),
        TaskStatus::Merging => "合并中".to_string(),
        TaskStatus::Completed => "已完成".to_string(),
        TaskStatus::Failed(err) => format!("失败: {}", err),
        TaskStatus::Cancelled => "已取消".to_string(),
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
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

pub(super) fn format_duration(seconds: f64) -> String {
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

pub(super) fn format_bitrate(size_bytes: u64, duration_seconds: f64) -> String {
    if duration_seconds <= 0.0 {
        return "0 bps".to_string();
    }
    let bps = (size_bytes as f64 * 8.0) / duration_seconds;
    if bps >= 1_000_000.0 {
        format!("{:.2} Mbps", bps / 1_000_000.0)
    } else if bps >= 1000.0 {
        format!("{:.2} Kbps", bps / 1000.0)
    } else {
        format!("{:.0} bps", bps)
    }
}
