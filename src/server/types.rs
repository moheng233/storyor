//! API 请求/响应类型集中定义
//!
//! 所有前后端共享的 API 数据结构在此定义，并标注 `#[derive(TS)]` 以通过
//! `cargo test` 自动导出为 TypeScript 类型定义（ts-rs）。
//!
//! Phase A 仅包含基础类型；Phase C 各路由端点会按需在自身模块或此处补充
//! 专属的请求/响应类型。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 统一错误响应体（所有 4xx/5xx 返回此结构）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorResponse {
    /// 错误消息（人类可读）
    pub error: String,
}

impl ErrorResponse {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { error: msg.into() }
    }
}

/// 健康检查响应
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    /// 服务名称
    pub service: String,
    /// 版本号
    pub version: String,
    /// 工作区目录
    pub workspace: String,
}

crate::register_ts!(ErrorResponse);
crate::register_ts!(HealthResponse);
