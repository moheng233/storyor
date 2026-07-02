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
//! 类型导出清单由各类型定义处的 `register_ts!(T)` 宏就近登记，
//! 经 `inventory` 全局注册表自动收集，测试端无需维护类型列表。
//! CI 可配合 `axfetchum::check()` 校验生成文件是否过期。

use ts_rs::Config;

/// 前端类型绑定导出目录（与 axfetchum 生成的 `api.ts` 同目录）
const BINDINGS_DIR: &str = "frontend/src/bindings";

#[test]
fn export_bindings() {
    // 构造 ts-rs 导出配置：显式指定输出目录，无需依赖 .cargo/config.toml 环境变量。
    let cfg = Config::new().with_out_dir(BINDINGS_DIR);

    // 导出所有已注册（`register_ts!`）的 TS 类型及其依赖到磁盘。
    // `TS::export_all` 会递归导出依赖，因此只需登记顶层入口类型。
    let count = storyor::ts_export::export_all(&cfg).expect("导出 TS 类型失败");
    eprintln!("✅ 已导出 {count} 个根 TS 类型到 {BINDINGS_DIR}/");

    // 触发 axfetchum 生成 TS API 客户端
    storyor::server::routes::export_bindings().expect("生成前端 TS 绑定失败");
    eprintln!("✅ 前端 TS API 客户端已生成到 {BINDINGS_DIR}/api.ts");
}
