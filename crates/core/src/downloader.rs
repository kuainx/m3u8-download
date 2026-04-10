use crate::config::{AppConfig, TempNameStrategy};
use crate::crypto;
use crate::merger;
use crate::parser::{self, Segment};
use futures::stream::{self, StreamExt};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONNECTION, HOST, UPGRADE_INSECURE_REQUESTS,
    USER_AGENT,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};

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

fn extract_host_header(url: &str) -> Option<HeaderValue> {
    url::Url::parse(url).ok().and_then(|parsed_url| {
        parsed_url.host_str().map(|host| {
            let host_header = if let Some(port) = parsed_url.port() {
                format!("{}:{}", host, port)
            } else {
                host.to_string()
            };
            HeaderValue::from_str(&host_header).ok()
        }).flatten()
    })
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
    pub estimated_total_size: Option<u64>,
    pub estimated_total_duration: Option<f64>,
    pub eta_seconds: Option<u64>,
}

/// 一个完整的下载任务
#[derive(Clone)]
pub struct DownloadTask {
    /// 任务 ID (M3U8 内容的 Hash)
    pub task_id: Arc<std::sync::Mutex<String>>,
    /// M3U8 URL
    pub url: String,
    /// 配置
    pub config: AppConfig,
    /// 进度信息（共享）
    pub progress: Arc<std::sync::Mutex<DownloadProgress>>,
    /// 取消标志
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// 输出文件名
    pub output_filename: String,
    /// 日志发送器
    pub log_sender: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
}

impl DownloadTask {
    /// 创建新的下载任务
    pub fn new(url: String, config: AppConfig, output_filename: String) -> Self {
        let task_id = Arc::new(std::sync::Mutex::new("pending".to_string()));
        let progress = Arc::new(std::sync::Mutex::new(DownloadProgress {
            status: TaskStatus::Pending,
            task_id: "pending".to_string(),
            output_path: None,
            estimated_total_size: None,
            estimated_total_duration: None,
            eta_seconds: None,
        }));
        Self {
            task_id,
            url,
            config,
            progress,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            output_filename,
            log_sender: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置日志发送器
    pub async fn set_log_sender(&self, sender: mpsc::UnboundedSender<String>) {
        let mut guard = self.log_sender.lock().await;
        *guard = Some(sender);
    }

    /// 取消任务
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.set_status(TaskStatus::Cancelled);
    }

    /// 是否已取消
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 更新进度
    pub fn set_status(&self, status: TaskStatus) {
        let mut prog = self.progress.lock().unwrap();
        // 如果已经是完成、取消或失败状态，则不再更新（避免后台残留任务覆盖最终状态）
        if matches!(
            prog.status,
            TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed(_)
        ) {
            return;
        }
        prog.status = status;
        prog.task_id = self.task_id.lock().unwrap().clone();
    }

    /// 获取当前进度
    pub fn get_progress(&self) -> DownloadProgress {
        self.progress.lock().unwrap().clone()
    }

    /// 执行下载任务（完整流程）
    pub async fn run(&self) -> Result<PathBuf, DownloadError> {
        match self.run_inner().await {
            Ok(path) => Ok(path),
            Err(e) => {
                if !matches!(e, DownloadError::Cancelled) {
                    self.set_status(TaskStatus::Failed(e.to_string()));
                }
                Err(e)
            }
        }
    }

    /// 内部执行逻辑
    async fn run_inner(&self) -> Result<PathBuf, DownloadError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"));
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));

        if let Some(host_value) = extract_host_header(&self.url) {
            headers.insert(HOST, host_value);
        }


        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .default_headers(headers)
            .build()?;

        // === 1. 解析 M3U8 ===
        self.set_status(TaskStatus::Parsing);

        let (playlist, raw_content) = parser::fetch_and_parse(&client, &self.url).await?;

        // 根据策略生成任务 ID
        let task_id = match self.config.temp_name_strategy {
            TempNameStrategy::ContentHash => {
                let mut hasher = Sha256::new();
                hasher.update(raw_content.as_bytes());
                let hash = hex::encode(hasher.finalize());
                hash[..12].to_string()
            }
            TempNameStrategy::Filename => {
                let url_path = self.url.split('?').next().unwrap_or(&self.url);
                let url_path = url_path.split('#').next().unwrap_or(url_path);
                std::path::Path::new(url_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("m3u8_task")
                    .to_string()
            }
        };

        {
            let mut id_guard = self.task_id.lock().unwrap();
            *id_guard = task_id.clone();
        }
        // 同步更新进度中的 task_id
        {
            let mut prog = self.progress.lock().unwrap();
            prog.task_id = task_id;
        }

        let total_segments = playlist.segments.len();
        let total_duration: f64 = playlist.segments.iter().map(|s| s.duration).sum();

        // 更新预估时长
        {
            let mut prog = self.progress.lock().unwrap();
            prog.estimated_total_duration = Some(total_duration);
        }

        // 创建临时目录
        let temp_dir = {
            let id = self.task_id.lock().unwrap();
            self.config.temp_dir.join(id.as_str())
        };
        fs::create_dir_all(&temp_dir).await?;

        // === 2. 获取所有 Key ===
        let key_cache = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));

        // === 3. 并发下载分片 ===
        if self.is_cancelled() {
            let _ = fs::remove_dir_all(&temp_dir).await;
            self.set_status(TaskStatus::Cancelled);
            return Err(DownloadError::Cancelled);
        }

        self.set_status(TaskStatus::Downloading {
            completed: 0,
            total: total_segments,
        });

        let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // === 预估文件大小：下载第一个分片并计算 ===
        if total_segments > 0 {
            let first_seg = &playlist.segments[0];
            let first_seg_path = merger::segment_path(&temp_dir, 0);

            let mut need_download = true;
            if first_seg_path.exists() {
                if let Ok(meta) = fs::metadata(&first_seg_path).await {
                    if meta.len() > 0 {
                        need_download = false;
                    }
                }
            }

            if need_download {
                download_segment(&client, first_seg, &first_seg_path, &key_cache).await?;
            }

            // 获取第一个分片的大小并预估总大小
            if let Ok(meta) = fs::metadata(&first_seg_path).await {
                let first_size = meta.len();
                let estimated_size = first_size * total_segments as u64;
                let mut prog = self.progress.lock().unwrap();
                prog.estimated_total_size = Some(estimated_size);
            }
        }

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
            });
        }

        let start_time = std::time::Instant::now();
        let initial_count = initial_completed;

        let segments: Vec<(usize, Segment)> = playlist.segments.into_iter().enumerate().collect();

        let progress_ref = self.progress.clone();
        let task_id_ref = self.task_id.clone();
        let cancelled_ref = self.cancelled.clone();
        let max_retries = self.config.max_retries;
        let has_error = Arc::new(std::sync::atomic::AtomicBool::new(false));

        stream::iter(segments)
            .for_each_concurrent(self.config.concurrent_downloads, |(i, segment)| {
                let client = client.clone();
                let temp_dir = temp_dir.clone();
                let key_cache = key_cache.clone();
                let completed_count = completed_count.clone();
                let progress_ref = progress_ref.clone();
                let task_id_ref = task_id_ref.clone();
                let cancelled_ref = cancelled_ref.clone();
                let has_error = has_error.clone();

                async move {
                    // 检查是否已取消或出错
                    if cancelled_ref.load(std::sync::atomic::Ordering::Relaxed)
                        || has_error.load(std::sync::atomic::Ordering::Relaxed)
                    {
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
                                let mut prog = progress_ref.lock().unwrap();
                                // 这里手动更新也是为了效率，但也得检查状态
                                if !matches!(
                                    prog.status,
                                    TaskStatus::Completed
                                        | TaskStatus::Cancelled
                                        | TaskStatus::Failed(_)
                                ) {
                                    prog.status = TaskStatus::Downloading {
                                        completed: done,
                                        total: total_segments,
                                    };
                                    prog.task_id = task_id_ref.lock().unwrap().clone();

                                    // 计算 ETA
                                    let elapsed = start_time.elapsed().as_secs_f64();
                                    let newly_completed = done.saturating_sub(initial_count);
                                    if elapsed > 1.0 && newly_completed > 0 {
                                        let speed = newly_completed as f64 / elapsed; // segments per second
                                        let remaining = total_segments.saturating_sub(done);
                                        prog.eta_seconds = Some((remaining as f64 / speed) as u64);
                                    }
                                }
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
                    has_error.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            })
            .await;

        if self.is_cancelled() {
            self.set_status(TaskStatus::Cancelled);
            return Err(DownloadError::Cancelled);
        }

        if has_error.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(DownloadError::MaxRetriesExceeded(
                "部分分片下载失败".to_string(),
            ));
        }

        // === 4. 合并分片 ===
        self.set_status(TaskStatus::Merging);

        let output_path = self.config.save_path.join(&self.output_filename);

        // 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let log_sender = {
            let guard = self.log_sender.lock().await;
            guard.clone()
        };

        merger::merge_segments(&temp_dir, &output_path, total_segments, log_sender).await?;

        // 清理临时文件
        let _ = merger::cleanup_temp(&temp_dir).await;

        // 更新完成状态
        {
            let mut prog = self.progress.lock().unwrap();
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
    let mut request_builder = client.get(&segment.url);
    if let Some(host_value) = extract_host_header(&segment.url) {
        request_builder = request_builder.header(HOST, host_value);
    }
    let response = request_builder.send().await?.error_for_status()?;
    let mut data = response.bytes().await?.to_vec();

    // 如果有加密，进行解密
    if let Some(ref key_info) = segment.key_info {
        // 获取或缓存 Key
        let key = {
            let mut cache = key_cache.lock().await;
            if let Some(cached_key) = cache.get(&key_info.uri) {
                cached_key.clone()
            } else {
                let mut key_request = client.get(&key_info.uri);
                if let Some(host_value) = extract_host_header(&key_info.uri) {
                    key_request = key_request.header(HOST, host_value);
                }
                let key_response = key_request.send().await?.error_for_status()?;
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
