//! 项目管理 CRUD
//!
//! 每个 storyor 项目独立存放在 `workspace_dir/<project_name>/` 目录下，
//! 元信息持久化到 `project.json`。本模块提供项目的增删查改与目录结构管理。
//!
//! 项目目录结构（与 v2 PLAN.md 一致）：
//! ```text
//! <workspace_dir>/<project_name>/
//! ├── project.json            # 项目元信息
//! ├── novel.txt              # 上传的原始小说文本
//! ├── chapters.json           # 章节切分结果
//! ├── summaries.json          # 章节摘要
//! ├── segments.json           # 剧情段切分
//! ├── characters/final.json  # 累积角色库
//! ├── voices/                # 音色设计产物
//! ├── scripts/               # 剧本产物
//! ├── audio/                 # 音频产物
//! ├── manifest.json
//! └── checkpoint.json
//! ```

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{Result, StoryorError};

// ---------------------------------------------------------------------------
// 项目元信息
// ---------------------------------------------------------------------------

/// 四阶段流水线的阶段标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ProjectPhase {
    /// 刚创建，尚未开始预处理
    Created,
    /// 预处理中（章节切分 / 摘要 / 段划分）
    Preprocessing,
    /// 剧本生成中
    Scripting,
    /// 音色设计中
    VoiceDesigning,
    /// 音频合成中
    Synthesizing,
    /// 全部完成
    Completed,
}

impl Default for ProjectPhase {
    fn default() -> Self {
        Self::Created
    }
}

/// 项目元信息（持久化到 `project.json`）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProjectMeta {
    /// 项目唯一 ID（UUID v4，同时作为目录名）
    pub id: String,
    /// 项目名称（用户可读）
    pub name: String,
    /// 原始小说文本文件的相对路径（相对于项目目录）
    pub novel_path: String,
    /// 创建时间（UTC）
    pub created_at: DateTime<Utc>,
    /// 当前所处阶段
    #[serde(default)]
    pub current_phase: ProjectPhase,
    /// 原始小说字符数
    #[serde(default)]
    pub novel_chars: usize,
}

impl ProjectMeta {
    /// 项目元信息文件名
    pub const FILENAME: &'static str = "project.json";

    /// 原始小说文本文件名
    pub const NOVEL_FILENAME: &'static str = "novel.txt";

    /// 构造新的项目元信息
    pub fn new(name: &str, novel_chars: usize) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            novel_path: Self::NOVEL_FILENAME.to_string(),
            created_at: Utc::now(),
            current_phase: ProjectPhase::Created,
            novel_chars,
        }
    }
}

// ---------------------------------------------------------------------------
// 项目列表条目（轻量，用于列表展示）
// ---------------------------------------------------------------------------

/// 项目列表条目（不含大体积产物，仅元信息）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProjectListItem {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub current_phase: ProjectPhase,
    pub novel_chars: usize,
}

impl From<&ProjectMeta> for ProjectListItem {
    fn from(m: &ProjectMeta) -> Self {
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            created_at: m.created_at,
            current_phase: m.current_phase,
            novel_chars: m.novel_chars,
        }
    }
}

/// 创建项目的请求体
#[derive(Debug, Clone, Deserialize, TS)]
pub struct CreateProjectRequest {
    /// 项目名称
    pub name: String,
    /// 原始小说文本
    pub novel_text: String,
}

crate::register_ts!(ProjectMeta);
crate::register_ts!(ProjectListItem);
crate::register_ts!(ProjectPhase);
crate::register_ts!(CreateProjectRequest);

// ---------------------------------------------------------------------------
// 项目管理器
// ---------------------------------------------------------------------------

/// 项目管理器：负责 `workspace_dir` 下所有项目的增删查改。
///
/// 线程安全通过内部 `&AppConfig` 的只读引用保证——所有写操作都在
/// `workspace_dir` 内进行，互不冲突。
#[derive(Debug, Clone)]
pub struct ProjectManager {
    /// 工作区根目录（所有项目存放根目录）
    workspace_dir: PathBuf,
}

impl ProjectManager {
    /// 从配置构造
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            workspace_dir: config.workspace.dir.clone(),
        }
    }

    /// 工作区根目录
    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    /// 项目目录路径
    pub fn project_dir(&self, id: &str) -> PathBuf {
        self.workspace_dir.join(id)
    }

    /// 项目元信息文件路径
    pub fn project_meta_path(&self, id: &str) -> PathBuf {
        self.project_dir(id).join(ProjectMeta::FILENAME)
    }

    /// 确保工作区根目录存在
    fn ensure_workspace(&self) -> Result<()> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        Ok(())
    }

    /// 列出所有项目（按创建时间倒序）
    pub fn list(&self) -> Result<Vec<ProjectListItem>> {
        self.ensure_workspace()?;
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&self.workspace_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let meta_path = path.join(ProjectMeta::FILENAME);
            if !meta_path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&meta_path)?;
            let meta: ProjectMeta = serde_json::from_str(&content)?;
            items.push(ProjectListItem::from(&meta));
        }
        // 按创建时间倒序（新→旧）
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    /// 获取单个项目元信息
    pub fn get(&self, id: &str) -> Result<ProjectMeta> {
        let path = self.project_meta_path(id);
        let content = std::fs::read_to_string(&path).map_err(|e| {
            StoryorError::Project(format!("项目 {id} 不存在或无法读取: {e}"))
        })?;
        Ok(serde_json::from_str(&content)?)
    }

    /// 创建新项目：分配 ID → 建目录 → 写 novel.txt → 写 project.json
    pub fn create(&self, name: &str, novel_text: &str) -> Result<ProjectMeta> {
        self.ensure_workspace()?;

        if name.trim().is_empty() {
            return Err(StoryorError::Project("项目名称不能为空".into()));
        }
        if novel_text.trim().is_empty() {
            return Err(StoryorError::Project("小说文本不能为空".into()));
        }

        let meta = ProjectMeta::new(name, novel_text.chars().count());
        let project_dir = self.project_dir(&meta.id);

        // 防止 ID 冲突（极低概率）
        if project_dir.exists() {
            return Err(StoryorError::Project(format!(
                "项目目录已存在（ID 冲突）: {}",
                meta.id
            )));
        }

        // 创建项目目录与子目录
        std::fs::create_dir_all(project_dir.join("characters"))?;
        std::fs::create_dir_all(project_dir.join("scripts"))?;
        std::fs::create_dir_all(project_dir.join("audio"))?;
        std::fs::create_dir_all(project_dir.join("voices"))?;

        // 写入原始小说文本
        std::fs::write(project_dir.join(&meta.novel_path), novel_text)?;

        // 写入项目元信息
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(self.project_meta_path(&meta.id), meta_json)?;

        tracing::info!(
            "已创建项目 {name}（id={id}，{chars} 字符）",
            id = meta.id,
            chars = meta.novel_chars
        );
        Ok(meta)
    }

    /// 更新项目元信息
    pub fn save(&self, meta: &ProjectMeta) -> Result<()> {
        let path = self.project_meta_path(&meta.id);
        let json = serde_json::to_string_pretty(meta)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// 更新项目阶段
    pub fn set_phase(&self, id: &str, phase: ProjectPhase) -> Result<ProjectMeta> {
        let mut meta = self.get(id)?;
        meta.current_phase = phase;
        self.save(&meta)?;
        Ok(meta)
    }

    /// 删除项目（连同整个目录）
    pub fn delete(&self, id: &str) -> Result<()> {
        let dir = self.project_dir(id);
        if !dir.exists() {
            return Err(StoryorError::Project(format!("项目 {id} 不存在")));
        }
        std::fs::remove_dir_all(&dir)?;
        tracing::info!("已删除项目 {id}");
        Ok(())
    }

    /// 项目目录是否存在
    pub fn exists(&self, id: &str) -> bool {
        self.project_meta_path(id).exists()
    }

    /// 读取项目原始小说文本
    pub fn read_novel(&self, id: &str) -> Result<String> {
        let meta = self.get(id)?;
        let path = self.project_dir(id).join(&meta.novel_path);
        Ok(std::fs::read_to_string(&path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pm() -> (ProjectManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pm = ProjectManager {
            workspace_dir: dir.path().to_path_buf(),
        };
        (pm, dir)
    }

    #[test]
    fn create_and_get_project() {
        let (pm, _dir) = temp_pm();
        let meta = pm.create("测试项目", "正文内容").unwrap();
        assert_eq!(meta.name, "测试项目");
        assert_eq!(meta.current_phase, ProjectPhase::Created);
        assert_eq!(meta.novel_chars, 4);

        let got = pm.get(&meta.id).unwrap();
        assert_eq!(got.name, meta.name);

        let novel = pm.read_novel(&meta.id).unwrap();
        assert_eq!(novel, "正文内容");
    }

    #[test]
    fn list_projects_sorted_by_created_at_desc() {
        let (pm, _dir) = temp_pm();
        let m1 = pm.create("项目一", "内容一").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let m2 = pm.create("项目二", "内容二").unwrap();

        let list = pm.list().unwrap();
        assert_eq!(list.len(), 2);
        // 新的在前
        assert_eq!(list[0].id, m2.id);
        assert_eq!(list[1].id, m1.id);
    }

    #[test]
    fn delete_project_removes_directory() {
        let (pm, _dir) = temp_pm();
        let meta = pm.create("待删除", "内容").unwrap();
        assert!(pm.exists(&meta.id));
        pm.delete(&meta.id).unwrap();
        assert!(!pm.exists(&meta.id));
    }

    #[test]
    fn set_phase_updates_and_saves() {
        let (pm, _dir) = temp_pm();
        let meta = pm.create("阶段测试", "内容").unwrap();
        let updated = pm.set_phase(&meta.id, ProjectPhase::Preprocessing).unwrap();
        assert_eq!(updated.current_phase, ProjectPhase::Preprocessing);

        let reloaded = pm.get(&meta.id).unwrap();
        assert_eq!(reloaded.current_phase, ProjectPhase::Preprocessing);
    }
}
