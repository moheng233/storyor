//! `OpenAiClient` 专用错误类型
//!
//! 细化所有来自 HTTP / 序列化的失败原因，不再用 `StoryorError::Llm(String)` 拼接字符串。
//! 通过 `From<OpenAiError> for StoryorError` 自动向上转换为全局错误。

use crate::error::StoryorError;

/// OpenAI 客户端错误
#[derive(Debug, thiserror::Error)]
pub enum OpenAiError {
    /// 网络 / 传输层错误
    #[error("请求发送失败: {0}")]
    RequestFailed(#[from] reqwest::Error),

    /// HTTP 状态码非 2xx
    #[error("服务返回 {status}: {message}")]
    BadStatus {
        status: reqwest::StatusCode,
        message: String,
    },

    /// 响应反序列化失败
    #[error("响应解析失败: {source}; body: {body}")]
    Deserialize {
        source: serde_json::Error,
        body: String,
    },

    /// SSE 流式响应解析失败
    #[error("SSE 解析失败: {0}")]
    SseParse(String),

    /// 响应中没有文本内容
    #[error("响应无文本内容")]
    NoText,

    /// 响应中没有音频数据
    #[error("响应中未找到 audio.data 字段")]
    NoAudioData,
}

impl From<OpenAiError> for StoryorError {
    fn from(e: OpenAiError) -> Self {
        StoryorError::Llm(e.to_string())
    }
}
