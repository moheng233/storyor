//! chat 接口 TTS 封装 + 音频提取策略
//!
//! `TtsClient` 封装。`synthesize_line(speaker, content, description, library)` 对每条台词
//! 构造单轮 chat 消息：user 为导演模式描述 + 角色库音色设定，assistant 为台词文本。
//!
//! HTTP 调用与 `/chat/completions` 请求/响应解析统一走 [`OpenAiClient::chat_audio`]，
//! 本模块仅负责：消息构造、`audio` 配置传参、从 `choices[0].message.audio.data`
//! 做 base64 解码为 `Vec<u8>`。音频格式由 CLI `--audio-format` 指定（默认 mp3）。

use base64::Engine;
use tracing::{debug, info};

use crate::config::AppConfig;
use crate::error::{Result, StoryorError};
use crate::llm::{ChatMessage, OpenAiClient};
use crate::script::CharacterLibrary;

// ---------------------------------------------------------------------------
// TTS 客户端
// ---------------------------------------------------------------------------

/// TTS 客户端：复用 [`OpenAiClient`] 调用 chat 接口并提取 base64 音频
pub struct TtsClient {
    client: OpenAiClient,
    audio_format: String,
    /// 默认音色（保留供未来在请求体 `audio.voice` 中使用，当前由 guidance 注入消息）
    #[allow(dead_code)]
    voice: String,
    timeout_secs: u64,
}

impl TtsClient {
    /// 从配置构造 TTS 客户端
    pub fn new(config: &AppConfig) -> Result<Self> {
        let client =
            OpenAiClient::with_options(&config.tts_model, None, config.tts_timeout_secs)?;
        Ok(Self {
            client,
            audio_format: config.audio_format.clone(),
            voice: config.tts_voice.clone(),
            timeout_secs: config.tts_timeout_secs,
        })
    }

    /// 合成单句台词的音频
    ///
    /// 每次调用只包含一对 user/assistant 消息：
    /// - user：导演模式描述 + 角色库音色设定（统一角色音色）
    /// - assistant：LLM 生成的完整台词（已内联情绪/动作标注）
    pub async fn synthesize_line(
        &self,
        speaker: &str,
        content: &str,
        description: &str,
        library: &CharacterLibrary,
    ) -> Result<Vec<u8>> {
        let guidance = library
            .get(speaker)
            .map(|c| c.guidance.as_str())
            .unwrap_or("");

        let user_prompt = if guidance.is_empty() {
            description.to_string()
        } else {
            format!("{description}\n音色设定：{guidance}")
        };

        let messages = vec![
            ChatMessage::user(user_prompt),
            ChatMessage::assistant(content.to_string()),
        ];

        debug!(
            "TTS 请求：{} \"{}\"",
            speaker,
            content.chars().take(40).collect::<String>()
        );

        // 保持与原实现一致：请求体里 voice 字段为 None，
        // 实际音色通过 guidance 注入 assistant 消息与 user prompt。
        let resp = self
            .client
            .chat_audio(&messages, "mp3", None, None, None)
            .await?;

        let audio = resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.audio)
            .ok_or_else(|| StoryorError::Llm("TTS 响应中未找到 audio.data 字段".into()))?;

        let audio_bytes = base64::engine::general_purpose::STANDARD
            .decode(&audio.data)
            .map_err(StoryorError::from)?;

        info!(
            "TTS 合成完成：{} \"{}\" {} 字节，格式 {}",
            speaker,
            content.chars().take(30).collect::<String>(),
            audio_bytes.len(),
            self.audio_format
        );
        Ok(audio_bytes)
    }

    /// 音频格式扩展名
    pub fn audio_format(&self) -> &str {
        &self.audio_format
    }

    /// 超时秒数
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }
}
