//! storyor —— 小说评书朗读生成器
//!
//! CLI 入口：解析 CLI → 加载配置 → 构建 3 个 LLM provider（小/大/TTS）
//! → 运行 pipeline。

use clap::Parser;
use llm::builder::LLMBuilder;
use llm::chat::ChatProvider;
use tracing_subscriber::EnvFilter;

mod audio;
mod character;
mod checkpoint;
mod config;
mod error;
mod novel;
mod pipeline;
mod prompts;
mod script;
mod tts;

use config::Cli;
use error::{Result, StoryorError};
use tts::client::TtsClient;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("storyor=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    // 加载配置
    let mut config = config::load_config(&cli.config).map_err(|e| {
        tracing::error!("加载配置失败 {path:?}: {e}", path = cli.config);
        e
    })?;
    cli.apply_overrides(&mut config);

    tracing::info!("配置加载完成：输出目录 {:?}", config.output_dir);

    // 读取小说文本
    let novel_text = std::fs::read_to_string(&cli.input).map_err(StoryorError::from)?;
    tracing::info!("已读取小说：{:?}（{} 字符）", cli.input, novel_text.len());

    // 构建小模型 provider（章节摘要）
    let small_provider = build_provider(&config.small_model, None)?;
    // 构建大模型 provider（剧情段切分 + 剧本生成）
    let large_provider = build_provider(&config.large_model, Some(script::script_schema()))?;

    // 构建 TTS 客户端（不走 trait，直接 HTTP）
    let tts_client = TtsClient::new(&config)?;

    // 运行流水线
    let pipeline = pipeline::Pipeline::new(
        &config,
        small_provider.as_ref(),
        large_provider.as_ref(),
        &tts_client,
    );
    pipeline.run(&novel_text, cli.resume, cli.force).await?;

    Ok(())
}

/// 构建 LLM provider（带 resilient + validator）
fn build_provider(
    model_cfg: &config::ModelConfig,
    schema: Option<llm::chat::StructuredOutputFormat>,
) -> Result<Box<dyn ChatProvider>> {
    let backend = model_cfg.parse_backend().map_err(StoryorError::Config)?;

    let mut builder = LLMBuilder::new()
        .backend(backend)
        .model(&model_cfg.model)
        .resilient(true)
        .resilient_attempts(3)
        .resilient_backoff(500, 8000);

    if let Some(key) = &model_cfg.api_key {
        builder = builder.api_key(key);
    }
    if let Some(url) = &model_cfg.base_url {
        builder = builder.base_url(url);
    }
    if let Some(schema) = schema {
        builder = builder.schema(schema).validator_attempts(2);
    }

    let provider = builder
        .build()
        .map_err(|e| StoryorError::Llm(format!("构建 provider 失败: {e}")))?;
    Ok(provider)
}
