//! TypeScript 类型导出注册表
//!
//! 基于 [`inventory`] 的全局分布式注册表，让每个带 `#[derive(TS)]` 的类型
//! 就近声明「我要被导出」，测试端一行调用即可导出全部，无需集中维护类型列表。
//!
//! ## 用法
//!
//! 在类型定义所在模块的末尾（或定义之后）写一行：
//!
//! ```ignore
//! use crate::register_ts;
//!
//! #[derive(TS)]
//! pub struct Foo { ... }
//!
//! register_ts!(Foo);
//! register_ts!(Bar); // 枚举、结构体均可
//! ```
//!
//! 测试端：
//!
//! ```ignore
//! let cfg = ts_rs::Config::new().with_out_dir("frontend/src/bindings");
//! crate::ts_export::export_all(&cfg).expect("导出 TS 类型失败");
//! ```
//!
//! ## 原理
//!
//! `inventory::collect!` 声明一个全局集合，`inventory::submit!` 在链接期
//! 把条目写入 `.init_array` 段，运行时 `inventory::iter` 遍历所有条目。
//! 无初始化时序问题，比 `OnceLock + Mutex` 更可靠。

use ts_rs::ExportError;

/// 一个「可导出的 TS 类型」注册条目
///
/// 存放类型名（用于错误信息）和导出函数指针。
/// `inventory` 要求条目是 `'static` 且 `Send + Sync`：
/// `&'static` 引用天然满足，`fn` 指针也天然 `Send + Sync`。
/// 若类型不满足，`inventory::collect!` 会在编译期报错，无需额外自检。
#[derive(Debug)]
pub struct TsExportable {
    /// 类型名（人类可读，用于错误定位）
    pub name: &'static str,
    /// 导出函数：调用 `T::export_all(cfg)` 写入磁盘
    pub export: fn(&ts_rs::Config) -> Result<(), ExportError>,
}

// 全局集合：声明一次，在任意模块用 `inventory::submit!` 注册条目
// 若 TsExportable 不满足 Send + Sync，此处会编译失败
inventory::collect!(&'static TsExportable);

/// 注册一个 TS 类型到全局导出注册表
///
/// 在任意位置调用 `register_ts!(MyType);` 即可把该类型登记为「需导出」。
/// 必须在 `crate::ts_export` 模块之外调用（`submit!` 与 `collect!` 不能同文件）。
///
/// # 示例
///
/// ```ignore
/// #[derive(TS)]
/// pub struct ProjectMeta { ... }
///
/// crate::register_ts!(ProjectMeta);
/// ```
#[macro_export]
macro_rules! register_ts {
    ($ty:ty) => {
        ::inventory::submit! {
            &$crate::ts_export::TsExportable {
                name: ::std::stringify!($ty),
                export: |cfg| <$ty as ::ts_rs::TS>::export_all(cfg),
            }
        }
    };
}

/// 导出所有已注册类型到磁盘
///
/// 遍历全局注册表，对每个类型调用 `TS::export_all`。
/// `export_all` 会递归导出该类型及其所有依赖（如 `DateTime`、枚举变体），
/// 因此重复注册同一类型是安全的（会重复写入相同内容），通常无需去重。
///
/// # 参数
/// - `cfg`: ts-rs 导出配置（通过 `Config::new().with_out_dir(...)` 构造）
///
/// # 返回
/// - 成功：`Ok(())`，并返回实际导出的类型数量
/// - 失败：`Err(Vec<(类型名, 错误)>)`，收集所有失败条目便于一次性排错
pub fn export_all(cfg: &ts_rs::Config) -> Result<usize, Vec<(String, ExportError)>> {
    let mut count = 0usize;
    let mut errors = Vec::new();

    for entry in inventory::iter::<&'static TsExportable> {
        match (entry.export)(cfg) {
            Ok(()) => count += 1,
            Err(e) => errors.push((entry.name.to_string(), e)),
        }
    }

    if errors.is_empty() {
        Ok(count)
    } else {
        Err(errors)
    }
}

// 让「忘记注册」的防御更友好：作为已注册类型依赖的类型，
// 会被 `export_all` 递归导出，因此 `register_ts!` 只需登记「顶层入口类型」。
// 但若某顶层类型完全忘记 `register_ts!`，它不会出现在 `inventory::iter` 里，
// 此时建议用 `axfetchum::check()` 在 CI 中兜底校验生成文件是否过期。
