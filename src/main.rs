//! storyor —— 小说评书朗读生成器
//!
//! CLI 入口：解析 CLI → 加载配置 → 启动 Web UI 服务器（v2 交互式工作流）。
//!
//! 服务器监听 `host:port`，前端访问即可操作四阶段流水线：
//! 预处理 → 剧本生成 → 音色设计 → 音频合成。

use clap::Parser;
use tracing_subscriber::EnvFilter;

use storyor::config::Cli;

#[tokio::main]
async fn main() -> storyor::error::Result<()> {
    // 安装 rustls crypto provider（reqwest 用了 rustls-no-provider feature）
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("storyor=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    // 加载配置
    let mut config = storyor::config::load_config(&cli.config).map_err(|e| {
        tracing::error!("加载配置失败 {path:?}: {e}", path = cli.config);
        e
    })?;
    cli.apply_overrides(&mut config);

    tracing::info!(
        "配置加载完成：服务器 http://{host}:{port}，工作区 {ws:?}",
        host = config.server.host,
        port = config.server.port,
        ws = config.workspace.dir
    );

    storyor::server::run_server(&config).await
}
