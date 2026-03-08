use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("无分片文件可合并")]
    NoSegments,
}

/// 合并所有 TS 分片为一个完整的 .ts 文件
///
/// * `temp_dir` - 临时分片文件所在目录
/// * `output_path` - 输出文件路径
/// * `segment_count` - 分片总数
pub async fn merge_segments(
    temp_dir: &Path,
    output_path: &Path,
    segment_count: usize,
) -> Result<(), MergeError> {
    if segment_count == 0 {
        return Err(MergeError::NoSegments);
    }

    let mut output_file = fs::File::create(output_path).await?;

    for i in 0..segment_count {
        let seg_path = temp_dir.join(format!("seg_{:06}.ts", i));
        if seg_path.exists() {
            let data = fs::read(&seg_path).await?;
            output_file.write_all(&data).await?;
        }
    }

    output_file.flush().await?;
    Ok(())
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
