use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 临时文件名生成策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TempNameStrategy {
    /// 1. m3u8内容哈希
    ContentHash,
    /// 2. m3u8文件名 (默认)
    #[default]
    Filename,
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 并发下载数
    pub concurrent_downloads: usize,
    /// 下载保存路径
    pub save_path: PathBuf,
    /// 失败重试次数
    pub max_retries: u32,
    /// 临时文件目录
    pub temp_dir: PathBuf,
    /// 临时文件名策略
    pub temp_name_strategy: TempNameStrategy,
}

impl Default for AppConfig {
    fn default() -> Self {
        let save_path = dirs_default_download();
        let temp_dir = save_path.join(".m3u8_temp");
        Self {
            concurrent_downloads: 4,
            save_path,
            max_retries: 3,
            temp_dir,
            temp_name_strategy: TempNameStrategy::default(),
        }
    }
}

impl AppConfig {
    /// 从 JSON 文件加载配置
    pub fn load(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// 保存配置到 JSON 文件
    pub fn save(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

fn dirs_default_download() -> PathBuf {
    PathBuf::from(".m3u8_download")
}
