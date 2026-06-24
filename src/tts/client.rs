//! chat 接口 TTS 封装 + 音频提取策略
//!
//! `TtsClient` 封装。`synthesize_paragraph(paragraph, library)` 对每个 `Paragraph`
//! 构造多轮 chat 消息 `[assistant(guidance), user(text), ...]`。
//!
//! **不走 `ChatProvider::chat()` 的 trait 抽象**——直接调用底层 HTTP 客户端
//! 并反序列化原始 JSON，提取 `choices[0].message.audio.data` 做 base64 解码为 `Vec<u8>`。
//! 音频格式由 CLI `--audio-format` 指定（默认 mp3）。

use base64::Engine;
use serde::Deserialize;
use tracing::{debug, info};

use crate::config::AppConfig;
use crate::script::CharacterLibrary;
use crate::error::{Result, StoryorError};
use crate::script::Paragraph;

// ---------------------------------------------------------------------------
// 原始响应反序列化结构
// ---------------------------------------------------------------------------

/// OpenAI 风格 chat completion 响应（仅提取所需字段）
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    audio: Option<AudioData>,
}

#[derive(Debug, Deserialize)]
struct AudioData {
    data: String,
}

// ---------------------------------------------------------------------------
// 请求体构造
// ---------------------------------------------------------------------------

/// 请求消息
#[derive(Debug, serde::Serialize)]
struct RequestMessage {
    role: String,
    content: String,
}

#[derive(Debug, serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modalities: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// TTS 客户端
// ---------------------------------------------------------------------------

/// TTS 客户端：直接通过 HTTP 调用 chat 接口并提取 base64 音频
pub struct TtsClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    audio_format: String,
    timeout_secs: u64,
}

impl TtsClient {
    /// 从配置构造 TTS 客户端
    pub fn new(config: &AppConfig) -> Result<Self> {
        let tts = &config.tts_model;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.tts_timeout_secs))
            .build()?;

        let base_url = tts
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        Ok(Self {
            http,
            base_url,
            api_key: tts.api_key.clone(),
            model: tts.model.clone(),
            audio_format: config.audio_format.clone(),
            timeout_secs: config.tts_timeout_secs,
        })
    }

    /// 合成单个段落的音频
    ///
    /// 对 `Paragraph` 中的每句台词构造多轮 chat 消息：
    /// `[assistant(角色1.guidance), user(台词1), assistant(角色2.guidance), user(台词2), ...]`
    pub async fn synthesize_paragraph(
        &self,
        paragraph: &Paragraph,
        library: &CharacterLibrary,
    ) -> Result<Vec<u8>> {
        let messages = self.build_messages(paragraph, library)?;

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            // 请求音频输出（OpenAI 兼容格式）
            modalities: Some(vec!["text".to_string(), "audio".to_string()]),
        };

        debug!(
            "TTS 请求：段落 {}，{} 句台词",
            paragraph.index,
            paragraph.lines.len()
        );

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self.http.post(&url).json(&request);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| StoryorError::Llm(format!("TTS 请求失败: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| StoryorError::Llm(format!("TTS 响应读取失败: {e}")))?;

        if !status.is_success() {
            return Err(StoryorError::Llm(format!(
                "TTS 请求返回 {status}: {}",
                body.chars().take(500).collect::<String>()
            )));
        }

        let parsed: ChatCompletionResponse = serde_json::from_str(&body)
            .map_err(|e| StoryorError::Llm(format!("TTS 响应解析失败: {e}")))?;

        let audio = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.audio)
            .ok_or_else(|| StoryorError::Llm("TTS 响应中未找到 audio.data 字段".into()))?;

        let audio_bytes = base64::engine::general_purpose::STANDARD
            .decode(&audio.data)
            .map_err(StoryorError::from)?;

        info!(
            "TTS 合成完成：段落 {}，{} 字节，格式 {}",
            paragraph.index,
            audio_bytes.len(),
            self.audio_format
        );
        Ok(audio_bytes)
    }

    /// 构造多轮 chat 消息
    fn build_messages(
        &self,
        paragraph: &Paragraph,
        library: &CharacterLibrary,
    ) -> Result<Vec<RequestMessage>> {
        let mut messages = Vec::with_capacity(paragraph.lines.len() * 2);
        for line in &paragraph.lines {
            // assistant 消息：角色音色设定（guidance）
            let guidance = if line.speaker == "旁白" {
                "你是评书旁白，语调沉稳、节奏舒缓，负责交代背景与场景。".to_string()
            } else {
                library
                    .get(&line.speaker)
                    .map(|c| c.guidance.clone())
                    .unwrap_or_else(|| format!("你是角色{}，请用符合其身份的语气朗读。", line.speaker))
            };
            messages.push(RequestMessage {
                role: "assistant".to_string(),
                content: guidance,
            });
            // user 消息：台词文本（携带情绪标签）
            let text = if line.tags.is_empty() {
                line.content.clone()
            } else {
                format!("（情绪：{}）{}", line.tags.join("、"), line.content)
            };
            messages.push(RequestMessage {
                role: "user".to_string(),
                content: text,
            });
        }
        Ok(messages)
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
