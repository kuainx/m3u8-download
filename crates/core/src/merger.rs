use std::path::{Path, PathBuf};
use std::process::Stdio;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("无分片文件可合并")]
    NoSegments,
    #[error("FFmpeg合并失败，退出代码: {0}")]
    FfmpegError(i32),
    #[error("FFmpeg进程被意外终止")]
    FfmpegTerminated,
}

fn get_ffmpeg_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn find_ffmpeg() -> PathBuf {
    let name = get_ffmpeg_name();

    // 首先检查当前工作目录（通常是项目根目录）
    if let Ok(current_dir) = std::env::current_dir() {
        let root_path = current_dir.join(name);
        if root_path.exists() {
            return root_path;
        }
    }

    // 其次检查可执行文件所在目录
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        let path = exe_path.join(name);
        if path.exists() {
            return path;
        }
    }

    // 默认使用环境变量中的 ffmpeg
    PathBuf::from(name)
}

/// 合并所有 TS 分片为一个完整的 .mp4 文件
///
/// * `temp_dir` - 临时分片文件所在目录
/// * `output_path` - 输出文件路径
/// * `segment_count` - 分片总数
pub async fn merge_segments(
    temp_dir: &Path,
    output_path: &Path,
    segment_count: usize,
    log_sender: Option<mpsc::UnboundedSender<String>>,
) -> Result<(), MergeError> {
    if segment_count == 0 {
        return Err(MergeError::NoSegments);
    }

    // 生成 ffmpeg concat 需要的 file_list.txt
    let list_path = temp_dir.join("file_list.txt");
    let mut list_file = fs::File::create(&list_path).await?;

    for i in 0..segment_count {
        let seg_name = format!("seg_{:06}.ts", i);
        let line = format!("file '{}'\n", seg_name);
        list_file.write_all(line.as_bytes()).await?;
    }
    list_file.flush().await?;

    // 由于 Command 的 current_dir 是 temp_dir，需要将 output_path 转换为绝对路径
    let abs_output_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(output_path)
    };

    let ffmpeg_path = find_ffmpeg();

    let mut cmd = Command::new(&ffmpeg_path);
    cmd.current_dir(temp_dir)
        .arg("-y") // 覆盖输出文件
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-fflags")
        .arg("+genpts")
        .arg("-i")
        .arg("file_list.txt")
        .arg("-c")
        .arg("copy")
        .arg(&abs_output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        // 0x08000000 是 CREATE_NO_WINDOW 标志，防止在 Windows 上弹出命令行窗口
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd.spawn()?;

    let stderr = child.stderr.take().expect("Failed to open stderr");
    let sender = log_sender.clone();

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(ref s) = sender {
                let _ = s.send(format!("[FFmpeg] {}", line));
            }
        }
    });

    let status = child.wait().await?;

    if status.success() {
        Ok(())
    } else if let Some(code) = status.code() {
        Err(MergeError::FfmpegError(code))
    } else {
        Err(MergeError::FfmpegTerminated)
    }
}

/// 清理临时分片文件
pub async fn cleanup_temp(temp_dir: &Path) -> Result<(), MergeError> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir).await?;
    }
    Ok(())
}

/// 获取分片文件路径
pub fn segment_path(temp_dir: &Path, index: usize) -> PathBuf {
    temp_dir.join(format!("seg_{:06}.ts", index))
}
