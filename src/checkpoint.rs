//! 产物落盘 + 续跑检查
//!
//! 维护 `<output_dir>/checkpoint.json`，记录各阶段完成状态。
//! 启动时读取，校验 `novel_hash`/`config_hash`，按阶段+段索引跳过已完成产物。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, StoryorError};

// ---------------------------------------------------------------------------
// 状态结构
// ---------------------------------------------------------------------------

/// 阶段级完成状态
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    /// 未开始
    #[default]
    Pending,
    /// 已完成
    Done,
}

/// 段级完成状态（记录已完成索引集合）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SegmentStage {
    #[serde(default)]
    pub completed: Vec<usize>,
    pub total: usize,
}

impl SegmentStage {
    pub fn completed_set(&self) -> HashSet<usize> {
        self.completed.iter().copied().collect()
    }

    pub fn is_done(&self) -> bool {
        self.total > 0 && self.completed.len() >= self.total
    }

    pub fn mark_done(&mut self, index: usize) {
        if !self.completed.contains(&index) {
            self.completed.push(index);
            self.completed.sort_unstable();
        }
    }
}

/// 各阶段状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stages {
    #[serde(default)]
    pub chapters: StageStatus,
    #[serde(default)]
    pub summaries: StageStatus,
    #[serde(default)]
    pub segments: StageStatus,
    #[serde(default)]
    pub scripts: SegmentStage,
    #[serde(default)]
    pub audio: SegmentStage,
}

/// checkpoint 顶层结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Checkpoint {
    pub novel_hash: String,
    pub config_hash: String,
    pub stages: Stages,
}

// ---------------------------------------------------------------------------
// Checkpoint 管理器
// ---------------------------------------------------------------------------

/// checkpoint 管理器：负责读写 `<output_dir>/checkpoint.json`
pub struct CheckpointManager {
    path: PathBuf,
    state: Checkpoint,
    /// 是否忽略 checkpoint（--force）
    force: bool,
}

impl CheckpointManager {
    /// 加载或初始化 checkpoint
    pub fn load(output_dir: &Path, force: bool) -> Result<Self> {
        std::fs::create_dir_all(output_dir)?;
        let path = output_dir.join("checkpoint.json");
        let state = if force {
            Checkpoint::default()
        } else if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Checkpoint>(&content)?
        } else {
            Checkpoint::default()
        };
        Ok(Self { path, state, force })
    }

    /// 校验 novel_hash / config_hash；若变更则提示全量重跑
    ///
    /// 返回 `Ok(true)` 表示 hash 一致可续跑，`Ok(false)` 表示 hash 变更需重置，
    /// `Err` 表示首次运行（无 hash）。
    pub fn validate(&mut self, novel_hash: &str, config_hash: &str) -> Result<bool> {
        if self.force || self.state.novel_hash.is_empty() {
            // 首次运行或强制重跑：写入 hash
            self.state.novel_hash = novel_hash.to_string();
            self.state.config_hash = config_hash.to_string();
            return Ok(false);
        }
        let consistent = self.state.novel_hash == novel_hash
            && self.state.config_hash == config_hash;
        if !consistent {
            // hash 变更：重置状态
            self.state = Checkpoint {
                novel_hash: novel_hash.to_string(),
                config_hash: config_hash.to_string(),
                stages: Stages::default(),
            };
        }
        Ok(consistent)
    }

    pub fn chapters_done(&self) -> bool {
        self.state.stages.chapters == StageStatus::Done
    }

    pub fn summaries_done(&self) -> bool {
        self.state.stages.summaries == StageStatus::Done
    }

    pub fn segments_done(&self) -> bool {
        self.state.stages.segments == StageStatus::Done
    }

    pub fn mark_chapters_done(&mut self) {
        self.state.stages.chapters = StageStatus::Done;
    }

    pub fn mark_summaries_done(&mut self) {
        self.state.stages.summaries = StageStatus::Done;
    }

    pub fn mark_segments_done(&mut self) {
        self.state.stages.segments = StageStatus::Done;
    }

    pub fn scripts_total(&self) -> usize {
        self.state.stages.scripts.total
    }

    pub fn scripts_completed(&self) -> HashSet<usize> {
        self.state.stages.scripts.completed_set()
    }

    pub fn set_scripts_total(&mut self, total: usize) {
        self.state.stages.scripts.total = total;
    }

    pub fn mark_script_done(&mut self, index: usize) {
        self.state.stages.scripts.mark_done(index);
    }

    pub fn audio_total(&self) -> usize {
        self.state.stages.audio.total
    }

    pub fn audio_completed(&self) -> HashSet<usize> {
        self.state.stages.audio.completed_set()
    }

    pub fn set_audio_total(&mut self, total: usize) {
        self.state.stages.audio.total = total;
    }

    pub fn mark_audio_done(&mut self, index: usize) {
        self.state.stages.audio.mark_done(index);
    }

    /// 持久化到磁盘
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 哈希工具
// ---------------------------------------------------------------------------

/// 计算字符串的 SHA-256 哈希（十六进制）
pub fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in result {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 计算配置的哈希（取影响产物一致性的关键字段）
pub fn config_hash(config: &crate::config::AppConfig) -> String {
    let material = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        config.small_model.backend,
        config.small_model.model,
        config.large_model.backend,
        config.large_model.model,
        config.tts_model.backend,
        config.tts_model.model,
        config.chapter_regex
    );
    sha256_hex(&material)
}

/// 读取产物文件（若存在），用于续跑时跳过重新生成
pub fn read_artifact<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| StoryorError::Checkpoint(format!("读取产物失败 {path:?}: {e}")))?;
    let value = serde_json::from_str::<T>(&content)
        .map_err(|e| StoryorError::Checkpoint(format!("解析产物失败 {path:?}: {e}")))?;
    Ok(Some(value))
}

/// 写入产物文件
pub fn write_artifact<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_stage_mark_done() {
        let mut s = SegmentStage {
            completed: vec![],
            total: 3,
        };
        s.mark_done(1);
        s.mark_done(0);
        assert_eq!(s.completed, vec![0, 1]);
        assert!(!s.is_done());
        s.mark_done(2);
        assert!(s.is_done());
    }

    #[test]
    fn sha256_stable() {
        let h1 = sha256_hex("hello");
        let h2 = sha256_hex("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert_ne!(h1, sha256_hex("world"));
    }
}
