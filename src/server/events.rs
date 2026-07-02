//! 进度事件定义 + SSE 辅助
//!
//! 四阶段流水线的进度通过 `ProgressEvent` 表达，Phase B 之后由各阶段函数
//! 通过 `broadcast::Sender<ProgressEvent>` 推送，前端通过 SSE endpoint 订阅。
//!
//! 本模块目前仅定义事件类型与 SSE 辅助函数，实际推送在 Phase B 接入。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 流水线阶段标识（与 `ProjectPhase` 对齐，但用于事件上下文）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]

#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// 预处理（章节切分 / 摘要 / 段划分）
    Preprocess,
    /// 剧本生成
    Scripts,
    /// 音色设计
    Voices,
    /// 音频合成
    Audio,
}

/// 单次进度事件
///
/// 推送给前端以驱动 UI 进度展示（SSE）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressEvent {
    /// 阶段开始
    StageStarted {
        stage: Stage,
        #[serde(default)]
        total: Option<usize>,
    },
    /// 阶段内进度（已完成数 / 总数）
    StageProgress {
        stage: Stage,
        completed: usize,
        #[serde(default)]
        total: Option<usize>,
        #[serde(default)]
        message: Option<String>,
    },
    /// 阶段完成
    StageCompleted {
        stage: Stage,
    },
    /// 阶段出错
    StageError {
        stage: Stage,
        error: String,
    },
}
