//! axum Web UI 服务器（v2 交互式工作流入口）
//!
//! 提供四阶段流水线的 REST API + SSE 进度推送，前端访问
//! `http://{host}:{port}` 即可操作。
//!
//! 模块组织：
//! - [`state`] — 全局共享状态（`AppState`）
//! - [`types`] — API 请求/响应类型（`#[derive(TS)]` 自动导出 TS 类型）
//! - [`events`] — 进度事件定义与 SSE 辅助
//! - [`routes`] — 路由注册（`axfetchum::ApiRouter` builder 一次产出 axum Router 与 RouteCollection）

pub mod events;
pub mod routes;
pub mod state;
pub mod types;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::config::AppConfig;
use crate::error::Result;
use crate::project::ProjectManager;

use self::state::AppState;

/// 启动 axum 服务器
pub async fn run_server(config: &AppConfig) -> Result<()> {
    // 确保工作区根目录存在
    std::fs::create_dir_all(&config.workspace.dir)?;

    // 构建共享状态
    let project_manager = ProjectManager::from_config(config);
    let state = Arc::new(AppState::new(config.clone(), project_manager));

    // 构建路由
    let app = build_router(state.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::error::StoryorError::Server(format!("绑定 {addr} 失败: {e}")))?;
    info!("storyor 服务器启动：http://{addr}");
    info!("工作区目录：{ws:?}", ws = config.workspace.dir);

    axum::serve(listener, app)
        .await
        .map_err(|e| crate::error::StoryorError::Server(format!("服务器运行错误: {e}")))?;

    Ok(())
}

/// 构建 axum Router
///
/// 路由分为三层：
/// 1. `/api/*` — REST API 端点（Phase C 起逐步补充）
/// 2. `/health` — 健康检查
/// 3. `/*` — 静态文件服务（生产模式 serve `frontend/dist/`，开发模式走 Vite 代理）
pub fn build_router(state: Arc<AppState>) -> Router {
    // ApiRouter builder 一次产出真实路由 + 前端元数据（RouteCollection 暂存供导出）
    let (api_routes, _route_collection) = routes::api_router();
    let health_route = routes::health_router();

    Router::new()
        .merge(api_routes.with_state(state.clone()))
        .merge(health_route.with_state(state))
        // 开发阶段允许 Vite dev server (5173) 跨域访问
        .layer(CorsLayer::permissive())
        // 静态文件服务（生产模式下 serve frontend/dist）
        .fallback_service(routes::static_files_service())
}
