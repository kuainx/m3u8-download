# M3U8 Downloader

基于 Rust 和 egui 开发的高性能 M3U8 视频下载工具。

## 功能特性

- **高性能**：多线程异步并发下载视频分片（可配置并发数）
- **解密支持**：自动处理 AES-128 加密的视频流（自动获取 Key + IV）
- **断点续传**：以 M3U8 内容 Hash 为唯一标识，已下载分片不重复下载
- **现代 GUI**：基于 egui 的深色主题现代化界面
- **自动合并**：下载完成后自动按序合并为完整的 TS 文件
- **错误重试**：网络抖动时自动重试（可配置次数）
- **Master Playlist 支持**：自动解析多码率播放列表并选择最高码率

## 项目结构

```
m3u8-download/
├── Cargo.toml                  # Workspace 根配置
├── crates/
│   ├── core/                   # 核心逻辑库
│   │   └── src/
│   │       ├── lib.rs          # 模块导出
│   │       ├── parser.rs       # M3U8 解析
│   │       ├── downloader.rs   # 异步分片下载
│   │       ├── crypto.rs       # AES-128 解密
│   │       ├── merger.rs       # TS 分片合并
│   │       └── config.rs       # 应用配置
│   └── gui/                    # GUI 应用
│       └── src/
│           ├── main.rs         # 入口
│           └── app.rs          # egui 界面
├── GenAI.md                    # 开发规划文档
└── README.md
```

## 如何使用

### 环境要求

- [Rust](https://rustup.rs/) 1.70+

### 运行

```powershell
cargo run -p m3u8-downloader-gui --release
```

### 操作步骤

1. 输入 M3U8 播放列表的 URL
2. 设置输出文件名和保存路径
3. 调整并发下载数（默认 16 线程）
4. 点击 **开始下载**

## 技术栈

| 类别       | 技术               |
| :--------- | :----------------- |
| 语言       | Rust               |
| UI         | egui / eframe      |
| 异步运行时 | Tokio              |
| 网络请求   | Reqwest            |
| M3U8 解析  | m3u8-rs            |
| 解密       | aes + cbc          |
| 文件选择   | rfd                |

## License

MIT
