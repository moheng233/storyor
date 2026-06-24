//! 剧情段切分（大模型）
//!
//! 把所有章节摘要交给大模型，输出 `Vec<PlotSegment>`（含起止章节、剧情概述）。
//! 用 `StructuredOutputFormat` 保证 JSON。落盘 `<output>/segments.json`。

use llm::chat::{ChatMessage, ChatProvider};
use tracing::info;

use crate::checkpoint::{read_artifact, write_artifact};
use crate::config::AppConfig;
use crate::error::{Result, StoryorError};
use crate::script::{ChapterSummary, PlotSegment, SegmentList};

/// 剧情段切分阶段
pub struct SegmentStage<'a> {
    config: &'a AppConfig,
    large_model: &'a dyn ChatProvider,
}

impl<'a> SegmentStage<'a> {
    pub fn new(config: &'a AppConfig, large_model: &'a dyn ChatProvider) -> Self {
        Self { config, large_model }
    }

    /// 基于章节摘要切分剧情段
    pub async fn run(&self, summaries: &[ChapterSummary]) -> Result<Vec<PlotSegment>> {
        let output_path = self.config.output_dir.join("segments.json");
        if let Some(existing) = read_artifact::<Vec<PlotSegment>>(&output_path)?
            && !existing.is_empty()
        {
            info!("剧情段切分已存在，跳过（{} 段）", existing.len());
            return Ok(existing);
        }

        let prompt_template = crate::prompts::load("segment.md")?;

        // 构造摘要列表文本
        let summaries_text = summaries
            .iter()
            .map(|s| format!("第{}章：{}", s.chapter_index + 1, s.summary))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = prompt_template.replace("{{summaries}}", &summaries_text);

        info!("开始剧情段切分（{} 章摘要）", summaries.len());

        let messages = vec![
            ChatMessage::assistant()
                .content("你是剧情结构分析师，请将章节摘要按剧情弧线切分为若干剧情段。")
                .build(),
            ChatMessage::user().content(prompt).build(),
        ];

        let resp = self
            .large_model
            .chat(&messages)
            .await
            .map_err(|e| StoryorError::Llm(e.to_string()))?;

        let text = resp
            .text()
            .ok_or_else(|| StoryorError::Llm("剧情段切分响应无文本".into()))?;

        let segment_list: SegmentList = parse_json_response(&text)?;

        // 校验章节范围合法性
        let max_chapter = summaries.len();
        let mut segments = segment_list.segments;
        for seg in &mut segments {
            if seg.chapter_end >= max_chapter {
                seg.chapter_end = max_chapter.saturating_sub(1);
            }
            if seg.chapter_start > seg.chapter_end {
                seg.chapter_start = seg.chapter_end;
            }
        }

        write_artifact(&output_path, &segments)?;
        info!("剧情段切分完成，共 {} 段，已落盘 {:?}", segments.len(), output_path);
        Ok(segments)
    }
}

/// 解析大模型返回的 JSON（兼容 ```json 代码块包裹）
pub fn parse_json_response<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let trimmed = text.trim();
    let json_str = if let Some(stripped) = trimmed.strip_prefix("```json") {
        stripped.trim_end_matches("```").trim()
    } else if let Some(stripped) = trimmed.strip_prefix("```") {
        stripped.trim_end_matches("```").trim()
    } else {
        trimmed
    };
    serde_json::from_str::<T>(json_str).map_err(StoryorError::from)
}
