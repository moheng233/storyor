//! 章节摘要（小模型，并行）
//!
//! 用小模型对每章生成 1-2 句摘要。`buffer_unordered` 并发
//! （受 `max_concurrency` 限制）。结果落盘 `<output>/summaries.json`。

use futures::stream::{self, StreamExt};
use llm::chat::{ChatMessage, ChatProvider};
use tracing::info;

use crate::checkpoint::{read_artifact, write_artifact};
use crate::config::AppConfig;
use crate::error::{Result, StoryorError};
use crate::script::{Chapter, ChapterSummary};

/// 章节摘要生成器
pub struct SummaryStage<'a> {
    config: &'a AppConfig,
    small_model: &'a dyn ChatProvider,
}

impl<'a> SummaryStage<'a> {
    pub fn new(config: &'a AppConfig, small_model: &'a dyn ChatProvider) -> Self {
        Self { config, small_model }
    }

    /// 对所有章节生成摘要
    pub async fn run(&self, chapters: &[Chapter]) -> Result<Vec<ChapterSummary>> {
        let output_path = self.config.output_dir.join("summaries.json");
        if let Some(existing) = read_artifact::<Vec<ChapterSummary>>(&output_path)?
            && existing.len() == chapters.len()
        {
            info!("章节摘要已存在，跳过（{} 章）", existing.len());
            return Ok(existing);
        }

        let prompt_template = crate::prompts::load("summary.md")?;
        let concurrency = self.config.max_concurrency.max(1);

        info!("开始生成章节摘要（{} 章，并发 {}）", chapters.len(), concurrency);

        let results: Vec<Result<ChapterSummary>> = stream::iter(chapters.iter())
            .map(|chapter| {
                let prompt = prompt_template
                    .replace("{{title}}", &chapter.title)
                    .replace("{{content}}", &chapter.content);
                let small_model = self.small_model;
                async move {
                    let messages = vec![
                        ChatMessage::assistant()
                            .content("你是小说摘要助手，请用 1-2 句话概括章节核心剧情。")
                            .build(),
                        ChatMessage::user().content(prompt).build(),
                    ];
                    let resp = small_model
                        .chat(&messages)
                        .await
                        .map_err(|e| StoryorError::Llm(e.to_string()))?;
                    let summary = resp
                        .text()
                        .ok_or_else(|| StoryorError::Llm("摘要响应无文本".into()))?;
                    Ok::<ChapterSummary, StoryorError>(ChapterSummary {
                        chapter_index: chapter.index,
                        summary: summary.trim().to_string(),
                    })
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        let mut summaries = Vec::with_capacity(chapters.len());
        for res in results {
            summaries.push(res?);
        }
        // 按章节序号排序
        summaries.sort_by_key(|s| s.chapter_index);

        write_artifact(&output_path, &summaries)?;
        info!("章节摘要完成，已落盘 {:?}", output_path);
        Ok(summaries)
    }
}
