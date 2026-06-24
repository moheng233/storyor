//! storyor —— 小说评书朗读生成器
//!
//! CLI 入口：解析 CLI → 加载配置 → 构建 3 个 LLM provider（小/大/TTS）
//! → 运行 pipeline。

use clap::Parser;
use llm::builder::LLMBackend;
use llm::builder::LLMBuilder;
use llm::chat::ChatProvider;
use llm::providers::openai_compatible::{OpenAICompatibleProvider, OpenAIProviderConfig};
use serde_json::json;
use tracing_subscriber::EnvFilter;

use storyor::config::Cli;
use storyor::error::{Result, StoryorError};
use storyor::tts::client::TtsClient;

struct StoryorOpenAICompatConfig;

impl OpenAIProviderConfig for StoryorOpenAICompatConfig {
    const PROVIDER_NAME: &'static str = "OpenAICompatible";
    const DEFAULT_BASE_URL: &'static str = "https://api.openai.com/v1/";
    const DEFAULT_MODEL: &'static str = "gpt-4.1-mini";
    const SUPPORTS_REASONING_EFFORT: bool = false;
    const SUPPORTS_STRUCTURED_OUTPUT: bool = false;
    const SUPPORTS_PARALLEL_TOOL_CALLS: bool = false;
    const SUPPORTS_STREAM_OPTIONS: bool = true;
}

#[tokio::main]
async fn main() -> Result<()> {
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

    tracing::info!("配置加载完成：输出目录 {:?}", config.output_dir);

    // 读取小说文本
    let novel_text = std::fs::read_to_string(&cli.input).map_err(StoryorError::from)?;
    tracing::info!("已读取小说：{:?}（{} 字符）", cli.input, novel_text.len());

    // 构建小模型 provider（章节摘要）
    let small_provider = build_provider(&config.small_model, false)?;
    // 构建大模型 provider（剧情段切分 + 剧本生成）
    // xiaomimimo 兼容 OpenAI chat 接口，但不支持 json_schema，只支持 json_object。
    let large_provider = build_provider(&config.large_model, true)?;

    // 构建 TTS 客户端（不走 trait，直接 HTTP）
    let tts_client = TtsClient::new(&config)?;

    // 运行流水线
    let pipeline = storyor::pipeline::Pipeline::new(
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
    model_cfg: &storyor::config::ModelConfig,
    json_object_output: bool,
) -> Result<Box<dyn ChatProvider>> {
    let backend = model_cfg.parse_backend().map_err(StoryorError::Config)?;

    if backend == LLMBackend::OpenAI {
        let extra_body = if json_object_output {
            Some(json!({
                "response_format": {
                    "type": "json_object"
                }
            }))
        } else {
            None
        };

        let provider = OpenAICompatibleProvider::<StoryorOpenAICompatConfig>::new(
            model_cfg.api_key.clone().unwrap_or_default(),
            model_cfg.base_url.clone(),
            Some(model_cfg.model.clone()),
            model_cfg.max_tokens,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            extra_body,
            None,
            None,
            None,
            None,
        );
        return Ok(Box::new(provider));
    }

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
    if let Some(mt) = model_cfg.max_tokens {
        builder = builder.max_tokens(mt);
    }
    if json_object_output {
        builder = builder.extra_body(json!({
            "response_format": {
                "type": "json_object"
            }
        }));
    }

    let provider = builder
        .build()
        .map_err(|e| StoryorError::Llm(format!("构建 provider 失败: {e}")))?;
    Ok(provider)
}
