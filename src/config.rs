//! 三模型配置 + 全局参数 + CLI 定义
//!
//! 用 clap derive 暴露 CLI，包含三个模型（small/large/tts）的连接配置
//! 与全局流水线参数（正则、并发数、输出目录、段落长度上限等）。

use std::path::PathBuf;

use clap::Parser;
use llm::builder::LLMBackend;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 模型配置
// ---------------------------------------------------------------------------

/// 单个模型连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// 后端类型（OpenAI/DeepSeek/Ollama/...）
    pub backend: String,
    /// API Key
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL
    #[serde(default)]
    pub base_url: Option<String>,
    /// 模型标识
    pub model: String,
    /// 最大输出 token 数（None = 使用 API 默认值）
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl ModelConfig {
    /// 解析为 `LLMBackend` 枚举
    pub fn parse_backend(&self) -> Result<LLMBackend, String> {
        self.backend
            .parse::<LLMBackend>()
            .map_err(|e| format!("无法解析后端 `{}`: {e}", self.backend))
    }
}

// ---------------------------------------------------------------------------
// 全局配置
// ---------------------------------------------------------------------------

/// 全局流水线配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 小模型配置（章节摘要）
    pub small_model: ModelConfig,
    /// 大模型配置（剧情段切分 + 剧本生成）
    pub large_model: ModelConfig,
    /// TTS 模型配置（音频合成）
    pub tts_model: ModelConfig,
    /// 音色设计模型配置（voicedesign，生成参考音频）
    #[serde(default)]
    pub voice_design_model: Option<ModelConfig>,
    /// 音色克隆模型配置（voiceclone，基于参考音频 few-shot 克隆）
    #[serde(default)]
    pub voice_clone_model: Option<ModelConfig>,
    /// 章节切分正则（默认匹配「第X章」）
    #[serde(default = "default_chapter_regex")]
    pub chapter_regex: String,
    /// 摘要并发数
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    /// 输出目录（CLI 批处理模式使用；server 模式忽略，改用 workspace）
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// 段落长度上限（台词行数）
    #[serde(default = "default_max_paragraph_lines")]
    pub max_paragraph_lines: usize,
    /// 音频格式（mp3/wav/pcm/pcm16）
    #[serde(default = "default_audio_format")]
    pub audio_format: String,
    /// TTS 音色（默认 mimo_default）
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// TTS 请求超时（秒）
    #[serde(default = "default_tts_timeout")]
    pub tts_timeout_secs: u64,
    /// 服务器配置（Web UI 模式）
    #[serde(default)]
    pub server: ServerConfig,
    /// 工作区配置（项目目录管理）
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    /// 停顿时长映射（wait 动作）
    #[serde(default)]
    pub timing: TimingConfig,
    /// 预置音效库配置
    #[serde(default)]
    pub sounds: SoundsConfig,
}

/// 服务器监听配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 监听地址
    #[serde(default = "default_server_host")]
    pub host: String,
    /// 监听端口
    #[serde(default = "default_server_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_server_host(),
            port: default_server_port(),
        }
    }
}

/// 工作区配置（所有项目根目录）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// 工作区根目录，每个项目在其中按名称分子目录
    #[serde(default = "default_workspace_dir")]
    pub dir: PathBuf,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            dir: default_workspace_dir(),
        }
    }
}

/// 停顿时长语义标签 → 实际秒数映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingConfig {
    /// 短停顿（逗号、换气）
    #[serde(default = "default_short_pause")]
    pub short_pause_secs: f64,
    /// 中停顿（句间、场景微转）
    #[serde(default = "default_medium_pause")]
    pub medium_pause_secs: f64,
    /// 长停顿（场景转换、悬念留白）
    #[serde(default = "default_long_pause")]
    pub long_pause_secs: f64,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            short_pause_secs: default_short_pause(),
            medium_pause_secs: default_medium_pause(),
            long_pause_secs: default_long_pause(),
        }
    }
}

impl TimingConfig {
    /// 根据语义标签返回实际秒数；未知标签返回中停顿
    pub fn secs_for(&self, duration: &str) -> f64 {
        match duration {
            "short" => self.short_pause_secs,
            "medium" => self.medium_pause_secs,
            "long" => self.long_pause_secs,
            other => {
                tracing::warn!("未知停顿标签 `{other}`，回退为 medium");
                self.medium_pause_secs
            }
        }
    }
}

/// 预置音效库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundsConfig {
    /// 音效库目录路径（含 index.toml 与音频文件）
    #[serde(default = "default_sounds_dir")]
    pub dir: PathBuf,
}

impl Default for SoundsConfig {
    fn default() -> Self {
        Self {
            dir: default_sounds_dir(),
        }
    }
}

fn default_server_host() -> String {
    "127.0.0.1".to_string()
}

fn default_server_port() -> u16 {
    3001
}

fn default_workspace_dir() -> PathBuf {
    PathBuf::from("./workspace")
}

fn default_sounds_dir() -> PathBuf {
    PathBuf::from("./assets/sounds")
}

fn default_short_pause() -> f64 {
    0.5
}

fn default_medium_pause() -> f64 {
    1.5
}

fn default_long_pause() -> f64 {
    3.0
}

fn default_chapter_regex() -> String {
    r"第[零一二三四五六七八九十百千0-9]{1,6}[章节回]".to_string()
}

fn default_concurrency() -> usize {
    4
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("./output")
}

fn default_max_paragraph_lines() -> usize {
    20
}

fn default_audio_format() -> String {
    "mp3".to_string()
}

fn default_tts_voice() -> String {
    "mimo_default".to_string()
}

fn default_tts_timeout() -> u64 {
    120
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// storyor —— 小说评书朗读生成器
///
/// 以 Web UI 服务器模式运行（v2 交互式工作流）。启动后监听
/// `host:port`，前端访问即可操作四阶段流水线。
#[derive(Parser, Debug)]
#[command(name = "storyor", version, about)]
pub struct Cli {
    /// 配置文件路径（TOML/JSON）
    #[arg(short, long, default_value = "storyor.toml")]
    pub config: PathBuf,
    /// 监听地址（覆盖配置文件）
    #[arg(long)]
    pub host: Option<String>,
    /// 监听端口（覆盖配置文件）
    #[arg(long)]
    pub port: Option<u16>,
    /// 工作区目录（覆盖配置文件，所有项目存放根目录）
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

impl Cli {
    /// 将 CLI 覆盖项应用到配置
    pub fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(host) = &self.host {
            config.server.host = host.clone();
        }
        if let Some(port) = self.port {
            config.server.port = port;
        }
        if let Some(ws) = &self.workspace {
            config.workspace.dir = ws.clone();
        }
    }
}

/// 从文件加载配置（支持 TOML 与 JSON，按扩展名判断）
pub fn load_config(path: &std::path::Path) -> Result<AppConfig, crate::error::StoryorError> {
    let content = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let config = match ext.as_str() {
        "json" => serde_json::from_str::<AppConfig>(&content)?,
        _ => {
            // 默认按 TOML 解析
            toml::from_str::<AppConfig>(&content)
                .map_err(|e| crate::error::StoryorError::Config(format!("TOML 解析失败: {e}")))?
        }
    };
    Ok(config)
}
