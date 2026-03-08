use thiserror::Error;
use url::Url;

/// 解析错误类型
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("M3U8 解析失败: {0}")]
    Parse(String),
    #[error("无效的 URL: {0}")]
    InvalidUrl(String),
}

/// 加密密钥信息
#[derive(Debug, Clone)]
pub struct KeyInfo {
    /// Key 的 URI
    pub uri: String,
    /// AES-128 密钥（下载后填充）
    pub key: Option<Vec<u8>>,
    /// 初始化向量
    pub iv: Option<Vec<u8>>,
}

/// 单个分片信息
#[derive(Debug, Clone)]
pub struct Segment {
    /// 分片的完整 URL
    pub url: String,
    /// 分片序号
    pub index: u64,
    /// 分片时长（秒）
    pub duration: f64,
    /// 加密信息（若有）
    pub key_info: Option<KeyInfo>,
}

/// 解析后的 M3U8 播放列表
#[derive(Debug, Clone)]
pub struct ParsedPlaylist {
    /// 所有分片列表
    pub segments: Vec<Segment>,
    /// 播放列表类型标识
    pub is_master: bool,
}

/// 将相对 URL 转换为绝对 URL
fn resolve_url(base: &Url, uri: &str) -> Result<String, ParseError> {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return Ok(uri.to_string());
    }
    base.join(uri)
        .map(|u| u.to_string())
        .map_err(|e| ParseError::InvalidUrl(e.to_string()))
}

/// 获取 M3U8 内容并解析
pub fn fetch_and_parse<'a>(
    client: &'a reqwest::Client,
    url: &'a str,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(ParsedPlaylist, String), ParseError>> + Send + 'a>,
> {
    Box::pin(async move {
        let base_url = Url::parse(url).map_err(|e| ParseError::InvalidUrl(e.to_string()))?;

        let response = client.get(url).send().await?.error_for_status()?;
        let body = response.text().await?;

        let parsed = m3u8_rs::parse_playlist_res(body.as_bytes())
            .map_err(|e| ParseError::Parse(format!("{:?}", e)))?;

        match parsed {
            m3u8_rs::Playlist::MasterPlaylist(master) => {
                // 自动选择最高带宽的流
                let best_variant = master
                    .variants
                    .iter()
                    .max_by_key(|v| v.bandwidth)
                    .ok_or_else(|| ParseError::Parse("Master playlist 中无可用流".into()))?;

                let variant_url = resolve_url(&base_url, &best_variant.uri)?;
                // 递归解析 Media Playlist
                return fetch_and_parse(client, &variant_url).await;
            }
            m3u8_rs::Playlist::MediaPlaylist(media) => {
                let mut segments = Vec::new();
                let mut current_key: Option<KeyInfo> = None;
                let media_sequence = media.media_sequence;

                for (i, seg) in media.segments.iter().enumerate() {
                    // 处理 Key 信息
                    if let Some(key) = &seg.key {
                        match key.method {
                            m3u8_rs::KeyMethod::AES128 => {
                                if let Some(ref uri) = key.uri {
                                    let key_url = resolve_url(&base_url, uri)?;
                                    let iv = key.iv.as_ref().map(|iv_str| parse_iv(iv_str));
                                    current_key = Some(KeyInfo {
                                        uri: key_url,
                                        key: None,
                                        iv,
                                    });
                                }
                            }
                            m3u8_rs::KeyMethod::None => {
                                current_key = None;
                            }
                            _ => {}
                        }
                    }

                    let seg_url = resolve_url(&base_url, &seg.uri)?;
                    let index = media_sequence + i as u64;

                    // 如果没有显式 IV，使用分片序号作为 IV
                    let key_info = current_key.as_ref().map(|k| {
                        let mut ki = k.clone();
                        if ki.iv.is_none() {
                            let mut iv = vec![0u8; 16];
                            let idx_bytes = index.to_be_bytes();
                            iv[8..16].copy_from_slice(&idx_bytes);
                            ki.iv = Some(iv);
                        }
                        ki
                    });

                    segments.push(Segment {
                        url: seg_url,
                        index,
                        duration: seg.duration as f64,
                        key_info,
                    });
                }

                return Ok((
                    ParsedPlaylist {
                        segments,
                        is_master: false,
                    },
                    body,
                ));
            }
        }
    })
}

/// 解析 IV 十六进制字符串 (例如 0x00000000000000000000000000000001)
fn parse_iv(iv_str: &str) -> Vec<u8> {
    let hex_str = iv_str.trim_start_matches("0x").trim_start_matches("0X");
    // 确保 16 字节
    let padded = format!("{:0>32}", hex_str);
    hex::decode(&padded).unwrap_or_else(|_| vec![0u8; 16])
}
