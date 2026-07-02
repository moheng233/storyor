//! OpenAI `/v1/chat/completions` 请求/响应类型
//!
//! 手动定义序列化结构，不依赖第三方 OpenAI SDK。
//! 仅包含本项目实际使用的字段；流式响应通过 `chat_stream` 解析 SSE。
//!
//! 参考：<https://platform.openai.com/docs/api-reference/chat>

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// 消息
// ---------------------------------------------------------------------------

/// 角色（OpenAI 兼容）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 单条消息。current 流水线只用到 system/user/assistant 三种角色。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    /// 是否为 user 消息（供 mock 记录断言使用）
    pub fn is_user(&self) -> bool {
        self.role == Role::User
    }
}

// ---------------------------------------------------------------------------
// 请求体
// ---------------------------------------------------------------------------

/// `POST /v1/chat/completions` 请求体
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// 结构化输出格式
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// 音频输出配置（TTS 场景：`audio` 字段）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioConfig>,
    /// 最大输出 token 数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 采样温度
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 流式输出（`true` 时响应为 SSE）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// 请求体 `audio` 字段（TTS 场景）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// 音频格式（如 `mp3`、`wav`、`pcm`）
    pub format: String,
    /// 指定音色（部分服务可省略）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

/// `response_format` 字段
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// 纯文本（默认，省略即可）
    Text,
    /// `json_object`（宽松 JSON 模式）
    JsonObject,
    /// `json_schema`（严格结构化输出）
    JsonSchema {
        json_schema: JsonSchemaFormat,
    },
}

/// json_schema 严格模式定义
///
/// 对应 OpenAI `response_format.json_schema` 子对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchemaFormat {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema 定义
    pub schema: Value,
    pub strict: bool,
}

// ---------------------------------------------------------------------------
// 非流式响应体
// ---------------------------------------------------------------------------

/// 完整响应（非流式）
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// 单个 choice
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub index: u32,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

/// assistant 消息（仅取 content）
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    /// TTS 场景返回的音频数据（base64）
    #[serde(default)]
    pub audio: Option<AudioData>,
}

/// 响应中的音频数据（含 base64 编码的 `data`）
#[derive(Debug, Clone, Deserialize)]
pub struct AudioData {
    pub data: String,
}

/// 用量统计
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)]
    #[allow(dead_code)]
    pub prompt_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub completion_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub total_tokens: u32,
}

impl ChatResponse {
    /// 取首个 choice 的文本内容
    pub fn text(&self) -> Option<String> {
        self.choices
            .first()
            .and_then(|c| c.message.content.clone())
    }
}

// ---------------------------------------------------------------------------
// 错误响应
// ---------------------------------------------------------------------------

/// OpenAI 错误响应体（HTTP 4xx/5xx）
#[derive(Debug, Deserialize)]
pub struct OpenAiErrorBody {
    pub error: OpenAiErrorDetail,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiErrorDetail {
    pub message: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub r#type: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub param: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub code: Option<String>,
}
