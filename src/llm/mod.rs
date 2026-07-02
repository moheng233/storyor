//! OpenAI 兼容 chat_completions 客户端
//!
//! 基于 `reqwest` 直接实现，不依赖 `llm` crate。提供三种调用方式：
//! - [`ChatClient::chat`] 普通非流式对话
//! - [`ChatClient::chat_with_schema`] json_schema 严格结构化输出（segment/script）
//! - [`ChatClient::chat_stream`] 流式对话（Web UI 聊天改稿，返回 content delta 流）
//!
//! 另提供 [`TtsClient`] trait，由 [`OpenAiClient`] 实现，用于 TTS 场景从
//! `choices[0].message.audio.data` 提取 base64 音频。
//!
//! 设计依据：OpenAI 官方 `/v1/chat/completions` 接口规范。

pub mod client;
pub mod error;
pub mod types;

pub use client::{OpenAiClient, OPENAI_DEFAULT_BASE_URL};
pub use types::{
    AudioConfig, AudioData, ChatCompletionRequest, ChatMessage, ChatResponse, Choice,
    JsonSchemaFormat, ResponseFormat, ResponseMessage, Role,
};

use std::pin::Pin;

use futures::Stream;

use crate::error::Result;

/// 流式对话产出的 token delta 流
///
/// 每项为一段 content delta 文本，或错误。
/// 生命周期 `'a` 绑定到产出该流的 `&self`。
pub type ChatDeltaStream<'a> =
    Pin<Box<dyn Stream<Item = Result<String>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// ChatClient trait：流水线与 server 共用
// ---------------------------------------------------------------------------

/// 统一的聊天客户端 trait：流水线与 server 共用，
/// 测试时可用 mock 实现注入（`tests/common/mod.rs`）。
#[async_trait::async_trait]
pub trait ChatClient: Send + Sync {
    /// 普通非流式对话（纯文本响应）。
    async fn chat(&self, messages: &[ChatMessage]) -> Result<ChatResponse>;

    /// json_schema 严格结构化输出对话（segment/script）。
    ///
    /// `schema` 为 `None` 时等价于 [`ChatClient::chat`]。
    async fn chat_with_schema(
        &self,
        messages: &[ChatMessage],
        schema: Option<&JsonSchemaFormat>,
    ) -> Result<ChatResponse>;

    /// 流式对话：返回 content delta 流。
    ///
    /// 仅供普通文本聊天（聊天改稿），不与 json_schema 同用。
    fn chat_stream(&self, messages: Vec<ChatMessage>) -> ChatDeltaStream<'_>;
}

// ---------------------------------------------------------------------------
// TtsClient trait：音频合成场景
// ---------------------------------------------------------------------------

/// TTS 客户端 trait：向 chat 接口发送带 `audio` 配置的请求，返回原始 [`ChatResponse`]。
///
/// 业务逻辑（guidance 注入、消息构造、base64 解码）由调用方负责。
/// 生产实现为 [`OpenAiClient`]（内部复用同一套 HTTP/鉴权/响应解析路径）。
#[async_trait::async_trait]
pub trait TtsClient: Send + Sync {
    /// 带 `audio` 输出的非流式对话，返回原始 [`ChatResponse`]。
    ///
    /// 调用方从 `choices[0].message.audio.data` 提取 base64 音频。
    async fn chat_audio(
        &self,
        messages: &[ChatMessage],
        audio_format: &str,
        voice: Option<&str>,
    ) -> Result<ChatResponse>;
}
