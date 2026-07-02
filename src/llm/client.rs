//! OpenAI 兼容客户端实现
//!
//! 基于 `reqwest` 直接发 HTTP 请求，同时实现 [`ChatClient`] 与 [`TtsClient`] 两个 trait。
//!
//! - [`ChatClient::chat`] —— 普通非流式对话
//! - [`ChatClient::chat_with_schema`] —— json_schema 严格结构化输出（segment/script）
//! - [`ChatClient::chat_stream`] —— 流式对话（Web UI 聊天改稿，返回 content delta 流）
//! - [`TtsClient::chat_audio`] —— TTS 场景，返回原始 [`ChatResponse`] 供调用方提取 `audio.data`
//!
//! 三类调用共享同一套 HTTP client、鉴权头、`/chat/completions` URL 构造与
//! `check_status` 错误处理路径。
//!
//! 内部使用 [`OpenAiError`] 严格类型化所有错误，通过 `From` 自动转换为全局 [`StoryorError`]。

use std::time::Duration;

use futures::{Stream, StreamExt};
use reqwest::Client;
use tracing::debug;

use crate::config::ModelConfig;
use crate::llm::error::OpenAiError;
use crate::llm::types::{
    AudioConfig, ChatCompletionRequest, ChatMessage, ChatResponse, JsonSchemaFormat,
    OpenAiErrorBody, ResponseFormat,
};
use crate::llm::{ChatClient, ChatDeltaStream, TtsClient};
use serde_json::Value;

type OpenAiResult<T> = std::result::Result<T, OpenAiError>;

/// OpenAI 默认 API base URL
pub const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI 兼容客户端
pub struct OpenAiClient {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiClient {
    /// 从 `ModelConfig` 构造（small/large/tts 等通用入口）。默认 120s 超时。
    pub fn new(config: &ModelConfig) -> crate::error::Result<Self> {
        Self::with_options(config, None, 120)
    }

    /// 带超时与时长上限的构造。
    pub fn with_options(
        config: &ModelConfig,
        timeout_override: Option<Duration>,
        default_secs: u64,
    ) -> crate::error::Result<Self> {
        let timeout = timeout_override.unwrap_or_else(|| Duration::from_secs(default_secs));
        let http = Client::builder().timeout(timeout).build()?;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| OPENAI_DEFAULT_BASE_URL.to_string());
        Ok(Self {
            http,
            base_url,
            api_key: config.api_key.clone(),
            model: config.model.clone(),
        })
    }

    // ---------- 请求构造 ----------

    fn build_request(
        &self,
        messages: &[ChatMessage],
        response_format: Option<ResponseFormat>,
    ) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            response_format,
            audio: None,
            max_tokens: None,
            temperature: None,
            stream: None,
        }
    }

    fn build_stream_request(&self, messages: &[ChatMessage]) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            response_format: None,
            audio: None,
            max_tokens: None,
            temperature: None,
            stream: Some(true),
        }
    }

    // ---------- HTTP 辅助 ----------

    fn url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn add_auth<'a>(&'a self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// 发送非流式请求，返回反序列化后的 [`ChatResponse`]。
    async fn send_chat(&self, request: &ChatCompletionRequest) -> OpenAiResult<ChatResponse> {
        debug!(model = %request.model, msgs = %request.messages.len(), "LLM 非流式请求");

        let resp = self
            .add_auth(self.http.post(self.url()).json(request))
            .send()
            .await?;

        let resp = check_status(resp).await?;

        let body_text = resp.text().await?;
        serde_json::from_str::<ChatResponse>(&body_text).map_err(|source| {
            OpenAiError::Deserialize {
                body: body_text.chars().take(500).collect(),
                source,
            }
        })
    }

    /// 发送流式请求，返回 `bytes_stream` 的 Response。
    async fn send_stream_req(
        &self,
        request: &ChatCompletionRequest,
    ) -> OpenAiResult<reqwest::Response> {
        let resp = self
            .add_auth(self.http.post(self.url()).json(request))
            .send()
            .await?;
        check_status(resp).await
    }
}

// ---------------------------------------------------------------------------
// ChatClient trait 实现
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ChatClient for OpenAiClient {
    async fn chat(&self, messages: &[ChatMessage]) -> crate::error::Result<ChatResponse> {
        self.chat_with_schema(messages, None).await
    }

    async fn chat_with_schema(
        &self,
        messages: &[ChatMessage],
        schema: Option<&JsonSchemaFormat>,
    ) -> crate::error::Result<ChatResponse> {
        let response_format = schema.map(|s| ResponseFormat::JsonSchema {
            json_schema: s.clone(),
        });
        let request = self.build_request(messages, response_format);
        Ok(self.send_chat(&request).await?)
    }

    fn chat_stream(&self, messages: Vec<ChatMessage>) -> ChatDeltaStream<'_> {
        let request = self.build_stream_request(&messages);
        Box::pin(build_delta_stream(self, request))
    }
}

// ---------------------------------------------------------------------------
// SSE delta 流
// ---------------------------------------------------------------------------

fn build_delta_stream(
    client: &OpenAiClient,
    request: ChatCompletionRequest,
) -> impl Stream<Item = crate::error::Result<String>> + '_ {
    async_stream::stream! {
        let resp = match client.send_stream_req(&request).await {
            Ok(r) => r,
            Err(e) => {
                yield Err(e.into());
                return;
            }
        };

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk: bytes::Bytes = match chunk {
                Ok(c) => c,
                Err(e) => {
                    yield Err(OpenAiError::RequestFailed(e).into());
                    return;
                }
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));

            // 按行解析 SSE
            while let Some(pos) = buf.find('\n') {
                let line: String = buf.drain(..=pos).collect::<String>().trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data == "[DONE]" {
                        return;
                    }
                    match serde_json::from_str::<Value>(data) {
                        Ok(v) => {
                            if let Some(delta) = v
                                .get("choices")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("delta"))
                                .and_then(|d| d.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                if !delta.is_empty() {
                                    yield Ok(delta.to_string());
                                }
                            }
                        }
                        Err(e) => {
                            yield Err(OpenAiError::SseParse(e.to_string()).into());
                            return;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TtsClient trait 实现
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl TtsClient for OpenAiClient {
    async fn chat_audio(
        &self,
        messages: &[ChatMessage],
        audio_format: &str,
        voice: Option<&str>,
    ) -> crate::error::Result<ChatResponse> {
        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            response_format: None,
            audio: Some(AudioConfig {
                format: audio_format.to_string(),
                voice: voice.map(|s| s.to_string()),
            }),
            max_tokens: None,
            temperature: None,
            stream: None,
        };
        Ok(self.send_chat(&request).await?)
    }
}

// ---------------------------------------------------------------------------
// HTTP 状态码校验
// ---------------------------------------------------------------------------

async fn check_status(resp: reqwest::Response) -> OpenAiResult<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<OpenAiErrorBody>(&text)
        .ok()
        .map(|b| b.error.message)
        .unwrap_or_else(|| text.chars().take(500).collect());
    Err(OpenAiError::BadStatus { status, message })
}
