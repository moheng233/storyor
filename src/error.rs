//! 统一错误类型

use thiserror::Error;

/// storyor 全局错误类型
#[derive(Debug, Error)]
pub enum StoryorError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("LLM 调用错误: {0}")]
    Llm(String),

    #[error("JSON 解析/序列化错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("正则表达式错误: {0}")]
    Regex(#[from] regex::Error),

    #[error("base64 解码错误: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("HTTP 请求错误: {0}")]
    Http(#[from] reqwest::Error),

    #[error("checkpoint 校验失败: {0}")]
    Checkpoint(String),

    #[error("配置错误: {0}")]
    Config(String),

    #[error("解析错误: {0}")]
    Parse(String),

    #[error("提示词模板缺失: {0}")]
    Prompt(String),
}

pub type Result<T> = std::result::Result<T, StoryorError>;
