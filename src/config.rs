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
    /// 章节切分正则（默认匹配「第X章」）
    #[serde(default = "default_chapter_regex")]
    pub chapter_regex: String,
    /// 摘要并发数
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    /// 输出目录
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// 段落长度上限（台词行数）
    #[serde(default = "default_max_paragraph_lines")]
    pub max_paragraph_lines: usize,
    /// 音频格式（mp3/wav/...）
    #[serde(default = "default_audio_format")]
    pub audio_format: String,
    /// TTS 请求超时（秒）
    #[serde(default = "default_tts_timeout")]
    pub tts_timeout_secs: u64,
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

fn default_tts_timeout() -> u64 {
    120
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// storyor —— 小说评书朗读生成器
///
/// 三阶段流水线：小模型逐章摘要 → 大模型切分剧情段并生成 JSON 剧本
/// → TTS 模型按段落合成音频。支持断点续跑、分段音频输出与清单管理。
#[derive(Parser, Debug)]
#[command(name = "storyor", version, about)]
pub struct Cli {
    /// 输入小说文本文件路径
    #[arg(short, long)]
    pub input: PathBuf,

    /// 配置文件路径（TOML/JSON）
    #[arg(short, long, default_value = "storyor.toml")]
    pub config: PathBuf,

    /// 输出目录（覆盖配置文件中的 output_dir）
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// 从最近断点继续
    #[arg(long, default_value_t = false)]
    pub resume: bool,

    /// 忽略 checkpoint，全量重跑
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// 音频格式（覆盖配置文件）
    #[arg(long)]
    pub audio_format: Option<String>,

    /// 摘要并发数（覆盖配置文件）
    #[arg(long)]
    pub concurrency: Option<usize>,

    /// 章节切分正则（覆盖配置文件）
    #[arg(long)]
    pub chapter_regex: Option<String>,
}

impl Cli {
    /// 将 CLI 覆盖项应用到配置
    pub fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(out) = &self.output {
            config.output_dir = out.clone();
        }
        if let Some(fmt) = &self.audio_format {
            config.audio_format = fmt.clone();
        }
        if let Some(c) = self.concurrency {
            config.max_concurrency = c;
        }
        if let Some(r) = &self.chapter_regex {
            config.chapter_regex = r.clone();
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
