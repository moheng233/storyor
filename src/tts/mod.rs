//! TTS 模块
//!
//! TTS 客户端实现已并入 [`crate::llm`]：[`crate::llm::OpenAiClient`] 同时实现
//! [`crate::llm::ChatClient`] 与 [`crate::llm::TtsClient`] 两个 trait。
//! 本模块仅保留为兼容入口指向。

pub use crate::llm::TtsClient;
