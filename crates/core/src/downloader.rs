use crate::config::AppConfig;
use crate::crypto;
use crate::merger;
use crate::parser::{self, Segment};
use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// 下载错误类型
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("解析失败: {0}")]
    Parse(#[from] parser::ParseError),
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("解密失败: {0}")]
    Crypto(#[from] crypto::CryptoError),
    #[error("合并失败: {0}")]
    Merge(#[from] merger::MergeError),
    #[error("任务已取消")]
    Cancelled,
    #[error("下载失败 (重试已用尽): {0}")]
    MaxRetriesExceeded(String),
}

/// 下载状态
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    /// 等待开始
    Pending,
    /// 正在解析 M3U8
    Parsing,
    /// 正在下载分片
    Downloading { completed: usize, total: usize },
    /// 正在合并
    Merging,
    /// 已完成
    Completed,
    /// 出错
    Failed(String),
    /// 已取消
    Cancelled,
}

/// 下载进度信息（用于 GUI 回调）
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub status: TaskStatus,
    pub task_id: String,
    pub output_path: Option<PathBuf>,
}

/// 一个完整的下载任务
#[derive(Clone)]
pub struct DownloadTask {
    /// 任务 ID (M3U8 内容的 Hash)
    pub task_id: String,
    /// M3U8 URL
    pub url: String,
    /// 配置
    pub config: AppConfig,
    /// 进度信息（共享）
    pub progress: Arc<Mutex<DownloadProgress>>,
    /// 取消标志
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// 输出文件名
    pub output_filename: String,
}

impl DownloadTask {
    /// 创建新的下载任务
    pub fn new(url: String, config: AppConfig, output_filename: String) -> Self {
        let task_id = "pending".to_string();
        let progress = Arc::new(Mutex::new(DownloadProgress {
            status: TaskStatus::Pending,
            task_id: task_id.clone(),
            output_path: None,
        }));
        Self {
            task_id,
            url,
            config,
            progress,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            output_filename,
        }
    }

    /// 取消任务
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 是否已取消
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 更新进度
    pub async fn set_status(&self, status: TaskStatus) {
        let mut prog = self.progress.lock().await;
        prog.status = status;
        prog.task_id = self.task_id.clone();
    }

    /// 获取当前进度
    pub async fn get_progress(&self) -> DownloadProgress {
        self.progress.lock().await.clone()
    }

    /// 执行下载任务（完整流程）
    pub async fn run(&mut self) -> Result<PathBuf, DownloadError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        // === 1. 解析 M3U8 ===
        self.set_status(TaskStatus::Parsing).await;

        let (playlist, raw_content) = parser::fetch_and_parse(&client, &self.url).await?;

        // 生成任务 ID (M3U8 内容 SHA-256 前 12 位)
        let mut hasher = Sha256::new();
        hasher.update(raw_content.as_bytes());
        let hash = hex::encode(hasher.finalize());
        self.task_id = hash[..12].to_string();

        let total_segments = playlist.segments.len();

        // 创建临时目录
        let temp_dir = self.config.temp_dir.join(&self.task_id);
        fs::create_dir_all(&temp_dir).await?;

        // === 2. 获取所有 Key ===
        let key_cache = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));

        // === 3. 并发下载分片 ===
        if self.is_cancelled() {
            self.set_status(TaskStatus::Cancelled).await;
            return Err(DownloadError::Cancelled);
        }

        self.set_status(TaskStatus::Downloading {
            completed: 0,
            total: total_segments,
        })
        .await;

        let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // 检查断点续传：已有的分片
        for i in 0..total_segments {
            let seg_path = merger::segment_path(&temp_dir, i);
            if seg_path.exists() {
                let metadata = fs::metadata(&seg_path).await?;
                if metadata.len() > 0 {
                    completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        // 更新已有进度
        let initial_completed = completed_count.load(std::sync::atomic::Ordering::Relaxed);
        if initial_completed > 0 {
            self.set_status(TaskStatus::Downloading {
                completed: initial_completed,
                total: total_segments,
            })
            .await;
        }

        let segments: Vec<(usize, Segment)> = playlist.segments.into_iter().enumerate().collect();

        let progress_ref = self.progress.clone();
        let task_id_ref = self.task_id.clone();
        let cancelled_ref = self.cancelled.clone();
        let max_retries = self.config.max_retries;

        stream::iter(segments)
            .for_each_concurrent(self.config.concurrent_downloads, |(i, segment)| {
                let client = client.clone();
                let temp_dir = temp_dir.clone();
                let key_cache = key_cache.clone();
                let completed_count = completed_count.clone();
                let progress_ref = progress_ref.clone();
                let task_id_ref = task_id_ref.clone();
                let cancelled_ref = cancelled_ref.clone();

                async move {
                    // 检查是否已取消
                    if cancelled_ref.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }

                    let seg_path = merger::segment_path(&temp_dir, i);

                    // 断点续传：跳过已下载的分片
                    if seg_path.exists() {
                        if let Ok(meta) = fs::metadata(&seg_path).await {
                            if meta.len() > 0 {
                                return; // 已下载
                            }
                        }
                    }

                    // 带重试的下载
                    let mut last_err = String::new();
                    for attempt in 0..=max_retries {
                        if cancelled_ref.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }

                        match download_segment(&client, &segment, &seg_path, &key_cache).await {
                            Ok(_) => {
                                let done = completed_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                // 更新进度
                                let mut prog = progress_ref.lock().await;
                                prog.status = TaskStatus::Downloading {
                                    completed: done,
                                    total: total_segments,
                                };
                                prog.task_id = task_id_ref.clone();
                                return;
                            }
                            Err(e) => {
                                last_err = e.to_string();
                                if attempt < max_retries {
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        500 * (attempt as u64 + 1),
                                    ))
                                    .await;
                                }
                            }
                        }
                    }

                    // 所有重试失败 - 记录到错误日志
                    eprintln!(
                        "分片 {} 下载失败 (已重试 {} 次): {}",
                        i, max_retries, last_err
                    );
                }
            })
            .await;

        if self.is_cancelled() {
            self.set_status(TaskStatus::Cancelled).await;
            return Err(DownloadError::Cancelled);
        }

        // === 4. 合并分片 ===
        self.set_status(TaskStatus::Merging).await;

        let output_path = self.config.save_path.join(&self.output_filename);

        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        merger::merge_segments(&temp_dir, &output_path, total_segments).await?;

        // 清理临时文件
        let _ = merger::cleanup_temp(&temp_dir).await;

        // 更新完成状态
        {
            let mut prog = self.progress.lock().await;
            prog.status = TaskStatus::Completed;
            prog.output_path = Some(output_path.clone());
        }

        Ok(output_path)
    }
}

/// 下载单个分片
async fn download_segment(
    client: &reqwest::Client,
    segment: &Segment,
    output_path: &Path,
    key_cache: &Arc<Mutex<HashMap<String, Vec<u8>>>>,
) -> Result<(), DownloadError> {
    // 下载分片数据
    let response = client.get(&segment.url).send().await?.error_for_status()?;
    let mut data = response.bytes().await?.to_vec();

    // 如果有加密，进行解密
    if let Some(ref key_info) = segment.key_info {
        // 获取或缓存 Key
        let key = {
            let mut cache = key_cache.lock().await;
            if let Some(cached_key) = cache.get(&key_info.uri) {
                cached_key.clone()
            } else {
                let key_response = client.get(&key_info.uri).send().await?.error_for_status()?;
                let key_data = key_response.bytes().await?.to_vec();
                cache.insert(key_info.uri.clone(), key_data.clone());
                key_data
            }
        };

        let iv = key_info
            .iv
            .as_ref()
            .ok_or_else(|| crypto::CryptoError::InvalidIvLength)?;

        data = crypto::decrypt_aes128(&data, &key, iv)?;
    }

    // 写入文件
    let mut file = fs::File::create(output_path).await?;
    file.write_all(&data).await?;
    file.flush().await?;

    Ok(())
}
