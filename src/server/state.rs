//! 全局共享状态
//!
//! [`AppState`] 在所有路由 handler 间共享，包含配置与项目管理器。
//! 通过 `Arc<AppState>` 以 axum `State` 提取器注入。

use crate::config::AppConfig;
use crate::project::ProjectManager;

/// 全局共享状态
///
/// 通过 `Arc` 共享，在 axum handler 中通过 `State<Arc<AppState>>` 提取。
#[derive(Debug)]
pub struct AppState {
    /// 全局配置
    config: AppConfig,
    /// 项目管理器（CRUD）
    project_manager: ProjectManager,
}

impl AppState {
    /// 构造共享状态
    pub fn new(config: AppConfig, project_manager: ProjectManager) -> Self {
        Self {
            config,
            project_manager,
        }
    }

    /// 全局配置
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// 项目管理器
    pub fn project_manager(&self) -> &ProjectManager {
        &self.project_manager
    }
}
