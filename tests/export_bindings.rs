//! 前后端类型绑定导出测试
//!
//! 运行 `cargo test --test export_bindings` 会：
//! 1. 通过 `ts-rs` 的 `#[derive(TS)]` + `#[ts(export)]` 把所有标注的 Rust 类型
//!    导出为 TypeScript 定义文件到 `frontend/src/bindings/`
//! 2. 通过 `axfetchum::generate_to_file` 用 `ApiRouter` builder 产出的路由元数据
//!    生成完整类型的前端 TS API 客户端 `frontend/src/bindings/api.ts`
//!
//! 这是 v2 架构的「单一来源」机制：Rust 端的 struct / 路由定义是唯一真相源，
//! 前端类型与 API 客户端全部自动生成，杜绝前后端类型漂移。
//!
//! CI 可配合 `axfetchum::check()` 校验生成文件是否过期。

use storyor::project::{CreateProjectRequest, ProjectListItem, ProjectMeta};
use storyor::server::routes;
use storyor::server::types::{ErrorResponse, HealthResponse};
use ts_rs::{Config, TS};

/// 前端类型绑定导出目录（与 axfetchum 生成的 `api.ts` 同目录）
const BINDINGS_DIR: &str = "frontend/src/bindings";

#[test]
fn export_bindings() {
    // 构造 ts-rs 导出配置：显式指定输出目录，无需依赖 .cargo/config.toml 环境变量。
    let cfg = Config::new().with_out_dir(BINDINGS_DIR);

    // 显式导出各根类型及其全部依赖（如 DateTime、各 enum 变体）。
    // `TS::export_all` 会递归把依赖类型也写入磁盘，因此只需调用顶层入口类型。
    // ProjectMeta 包含 ProjectPhase / DateTime，exports 后两者自动落盘。
    ProjectMeta::export_all(&cfg).expect("导出 ProjectMeta 失败");
    ProjectListItem::export_all(&cfg).expect("导出 ProjectListItem 失败");
    CreateProjectRequest::export_all(&cfg).expect("导出 CreateProjectRequest 失败");
    HealthResponse::export_all(&cfg).expect("导出 HealthResponse 失败");
    ErrorResponse::export_all(&cfg).expect("导出 ErrorResponse 失败");

    // 触发 axfetchum 生成 TS API 客户端
    if let Err(e) = routes::export_bindings() {
        panic!("生成前端 TS 绑定失败: {e}");
    }

    eprintln!("✅ 前端 TS 绑定已（重新）生成到 {BINDINGS_DIR}/");
}
