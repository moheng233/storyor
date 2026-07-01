//! 路由注册（axfetchum ApiRouter builder 模式）
//!
//! 使用 [`axfetchum::ApiRouter`] builder 一次定义同步产出两样东西：
//!
//! 1. **真实 axum `Router`**：可在 `serve` 时直接 `.with_state` 挂载
//! 2. **`RouteCollection` 元数据**：供 `generate_to_file` 生成前端 TS 客户端
//!
//! 这是 axfetchum 推荐的「Option A」用法，单一来源、零重复。
//! 详见 <https://github.com/yackey-labs/axfetchum#option-a-apirouter-builder-recommended>
//!
//! 新增/修改 API 端点时只需改这一处，并通过 `cargo test` 刷新前端绑定。
//!
//! Phase A 仅落地健康检查与项目管理骨架路由；Phase C 起逐步补充各阶段端点。

use std::sync::Arc;

use axfetchum::ApiRouter;
use axum::extract::State;
use axum::response::Json;
use axum::Router;
use tower_http::services::ServeDir;

use super::state::AppState;
use super::types::HealthResponse;
use crate::error::Result;
use crate::project::{CreateProjectRequest, ProjectListItem, ProjectMeta};

// ---------------------------------------------------------------------------
// 健康检查路由
// ---------------------------------------------------------------------------

/// 健康检查 axum Router
pub fn health_router() -> Router<Arc<AppState>> {
    Router::new().route("/health", axum::routing::get(health_handler))
}

/// `GET /health` — 服务健康检查
async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        service: "storyor".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        workspace: state.config().workspace.dir.display().to_string(),
    })
}

// ---------------------------------------------------------------------------
// API 路由（ApiRouter builder：一次定义，axum Router + RouteCollection 同步产出）
// ---------------------------------------------------------------------------

/// 构建所有 REST API 端点。
///
/// 返回元组 `(axum Router, RouteCollection)`：
/// - `Router` 在 [`super::serve`] 中挂载到应用，路径前缀 `/api`
/// - `RouteCollection` 经 `generate_to_file` 生成前端 TS 客户端
///
/// 路径参数统一使用 `{id}` 风格（axfetchum 元数据格式，axum 0.8 原生支持）。
#[allow(clippy::type_complexity)]
pub fn api_router() -> (Router<Arc<AppState>>, axfetchum::RouteCollection) {
    ApiRouter::<Arc<AppState>>::new()
        .group("projects")
        .get("/api/projects", projects_list)
            .response::<Vec<ProjectListItem>>()
            .done()
        .post("/api/projects", projects_create)
            .json::<CreateProjectRequest, ProjectMeta>()
            .done()
        .get("/api/projects/{id}", projects_get)
            .response::<ProjectMeta>()
            .done()
        .delete("/api/projects/{id}", projects_delete)
            .done()
        .build()
}

// ---------------------------------------------------------------------------
// 项目管理 handler（Phase A 基础 CRUD）
// ---------------------------------------------------------------------------

/// `GET /api/projects` — 项目列表
async fn projects_list(State(state): State<Arc<AppState>>) -> Result<Json<Vec<ProjectListItem>>> {
    let pm = state.project_manager();
    let list = pm.list()?;
    Ok(Json(list))
}

/// `POST /api/projects` — 创建项目
async fn projects_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectMeta>> {
    let pm = state.project_manager();
    let meta = pm.create(&req.name, &req.novel_text)?;
    Ok(Json(meta))
}

/// `GET /api/projects/{id}` — 获取项目详情
async fn projects_get(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ProjectMeta>> {
    let pm = state.project_manager();
    let meta = pm.get(&id)?;
    Ok(Json(meta))
}

/// `DELETE /api/projects/{id}` — 删除项目
async fn projects_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<()> {
    let pm = state.project_manager();
    pm.delete(&id)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 静态文件服务（生产模式 serve frontend/dist）
// ---------------------------------------------------------------------------

/// SPA 静态文件服务
///
/// 在生产模式下，axum 直接 serve `frontend/dist/`。
/// 开发模式下前端由 Vite dev server (5173) 提供，此 fallback 仅作为兜底。
pub fn static_files_service() -> ServeDir {
    ServeDir::new("frontend/dist")
}

// ---------------------------------------------------------------------------
// 前端 TS 客户端绑定生成
// ---------------------------------------------------------------------------

/// 默认绑定生成配置
///
/// 输出布局：
/// - `frontend/src/bindings/api.ts` — API 客户端（由 axfetchum 生成）
/// - `frontend/src/bindings/<Type>.ts` — 各类型定义（由 ts-rs 生成）
///
/// 因此 `type_import_prefix` 应为空：api.ts 与类型文件同目录，用 `./Type` 引用。
pub fn default_generator_config() -> axfetchum::GeneratorConfig {
    axfetchum::GeneratorConfig {
        bindings_dir: "frontend/src/bindings".into(),
        output_path: "frontend/src/bindings/api.ts".into(),
        factory_name: "createStoryorClient".into(),
        // 启用分组：api.projects.listProjects() 等命名空间
        enable_groups: true,
        error_class_name: "ApiError".into(),
        options_interface_name: "ClientOptions".into(),
        default_credentials: "same-origin".into(),
        // api.ts 与类型文件同目录，类型引用应为 "./TypeName"（即去掉前缀）
        type_import_prefix: ".".into(),
        format_command: None,
    }
}

/// 导出前后端绑定（ts-rs 类型 + axfetchum TS API 客户端）
///
/// 通过 `cargo test` 触发（见 `tests/export_bindings.rs`）。
/// 先导出 ts-rs 类型定义，再用路由元数据生成 API 客户端代码。
pub fn export_bindings() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_router, routes) = api_router();
    let config = default_generator_config();
    axfetchum::generate_to_file(&routes, &config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// builder 至少能成功构建出 4 条 projects 路由
    #[test]
    fn api_router_builds_without_error() {
        let (_router, routes) = api_router();
        assert!(routes.len() >= 4, "projects 路由应至少有 4 条：list/create/get/delete");
    }

    /// 导出配置可正常构造
    #[test]
    fn export_bindings_config_is_valid() {
        let _cfg = default_generator_config();
    }
}
