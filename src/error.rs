//! 统一错误类型

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
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

    #[error("项目管理错误: {0}")]
    Project(String),

    #[error("服务器错误: {0}")]
    Server(String),
}

impl From<axum::http::StatusCode> for StoryorError {
    fn from(code: axum::http::StatusCode) -> Self {
        StoryorError::Server(format!("HTTP 状态错误: {code}"))
    }
}

/// 将错误转换为统一的 JSON 响应体
///
/// 响应体格式：`{"error": "<message>"}`，状态码默认 500（Project 未找到为 404）。
impl IntoResponse for StoryorError {
    fn into_response(self) -> Response {
        // 项目未找到 → 404；其他归为 500 内部错误
        let is_not_found = matches!(&self, StoryorError::Project(msg) if msg.contains("未找到"));
        let status = if is_not_found {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, StoryorError>;
