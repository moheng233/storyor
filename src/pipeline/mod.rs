//! 流水线编排 + 断点续跑
//!
//! `Pipeline::run()` 串联全流程：chapters → summaries → segments → scripts → audio。
//! 每阶段/每段完成后写 checkpoint。支持 `--resume` 从最近断点继续。

use std::collections::HashSet;

use llm::chat::ChatProvider;
use tracing::info;

use crate::audio::AudioStage;
use crate::checkpoint::{config_hash, sha256_hex, CheckpointManager};
use crate::config::AppConfig;
use crate::error::{Result, StoryorError};
use crate::novel::ChapterSplitter;
use crate::script::{Chapter, ChapterSummary, PlotSegment, Script};
use crate::tts::client::TtsClient;

pub mod script;
pub mod segment;
pub mod summary;

/// 流水线
pub struct Pipeline<'a> {
    config: &'a AppConfig,
    small_model: &'a dyn ChatProvider,
    large_model: &'a dyn ChatProvider,
    tts_client: &'a TtsClient,
}

impl<'a> Pipeline<'a> {
    pub fn new(
        config: &'a AppConfig,
        small_model: &'a dyn ChatProvider,
        large_model: &'a dyn ChatProvider,
        tts_client: &'a TtsClient,
    ) -> Self {
        Self {
            config,
            small_model,
            large_model,
            tts_client,
        }
    }

    /// 运行完整流水线
    pub async fn run(&self, novel_text: &str, resume: bool, force: bool) -> Result<()> {
        let novel_hash = sha256_hex(novel_text);
        let cfg_hash = config_hash(self.config);

        let mut ckpt = CheckpointManager::load(&self.config.output_dir, force)?;
        let consistent = ckpt.validate(&novel_hash, &cfg_hash)?;
        if !consistent && resume {
            info!("novel/config 哈希变更，已重置 checkpoint，将全量重跑");
        }

        // ---- 阶段 1：章节切分 ----
        let chapters = self.stage_chapters(&mut ckpt, novel_text)?;

        // ---- 阶段 2：章节摘要 ----
        let summaries = self.stage_summaries(&mut ckpt, &chapters).await?;

        // ---- 阶段 3：剧情段切分 ----
        let segments = self.stage_segments(&mut ckpt, &summaries).await?;

        // ---- 阶段 4：剧本生成 ----
        let scripts = self.stage_scripts(&mut ckpt, &segments, &chapters).await?;

        // ---- 阶段 5：TTS 合成 ----
        self.stage_audio(&mut ckpt, &scripts).await?;

        ckpt.save()?;
        info!("流水线全部完成");
        Ok(())
    }

    /// 阶段 1：章节切分
    fn stage_chapters(
        &self,
        ckpt: &mut CheckpointManager,
        novel_text: &str,
    ) -> Result<Vec<Chapter>> {
        let path = self.config.output_dir.join("chapters.json");
        if ckpt.chapters_done()
            && let Some(existing) = crate::checkpoint::read_artifact::<Vec<Chapter>>(&path)?
        {
            info!("章节切分已完成，跳过（{} 章）", existing.len());
            return Ok(existing);
        }
        info!("开始章节切分");
        let splitter = ChapterSplitter::new(&self.config.chapter_regex)?;
        let chapters = splitter.split(novel_text);
        if chapters.is_empty() {
            return Err(StoryorError::Parse("章节切分结果为空".into()));
        }
        crate::checkpoint::write_artifact(&path, &chapters)?;
        ckpt.mark_chapters_done();
        ckpt.save()?;
        info!("章节切分完成，共 {} 章", chapters.len());
        Ok(chapters)
    }

    /// 阶段 2：章节摘要
    async fn stage_summaries(
        &self,
        ckpt: &mut CheckpointManager,
        chapters: &[Chapter],
    ) -> Result<Vec<ChapterSummary>> {
        if ckpt.summaries_done() {
            let path = self.config.output_dir.join("summaries.json");
            if let Some(existing) = crate::checkpoint::read_artifact::<Vec<ChapterSummary>>(&path)?
            {
                info!("章节摘要已完成，跳过");
                return Ok(existing);
            }
        }
        let stage = summary::SummaryStage::new(self.config, self.small_model);
        let summaries = stage.run(chapters).await?;
        ckpt.mark_summaries_done();
        ckpt.save()?;
        Ok(summaries)
    }

    /// 阶段 3：剧情段切分
    async fn stage_segments(
        &self,
        ckpt: &mut CheckpointManager,
        summaries: &[ChapterSummary],
    ) -> Result<Vec<PlotSegment>> {
        if ckpt.segments_done() {
            let path = self.config.output_dir.join("segments.json");
            if let Some(existing) = crate::checkpoint::read_artifact::<Vec<PlotSegment>>(&path)? {
                info!("剧情段切分已完成，跳过");
                return Ok(existing);
            }
        }
        let stage = segment::SegmentStage::new(self.config, self.large_model);
        let segments = stage.run(summaries).await?;
        ckpt.mark_segments_done();
        ckpt.set_scripts_total(segments.len());
        ckpt.set_audio_total(segments.len());
        ckpt.save()?;
        Ok(segments)
    }

    /// 阶段 4：剧本生成
    async fn stage_scripts(
        &self,
        ckpt: &mut CheckpointManager,
        segments: &[PlotSegment],
        chapters: &[Chapter],
    ) -> Result<Vec<Script>> {
        let completed: HashSet<usize> = ckpt.scripts_completed();
        let stage = script::ScriptStage::new(self.config, self.large_model);
        let scripts = stage.run(segments, chapters, &completed).await?;
        for seg in segments {
            ckpt.mark_script_done(seg.index);
        }
        ckpt.save()?;
        Ok(scripts)
    }

    /// 阶段 5：TTS 合成
    async fn stage_audio(
        &self,
        ckpt: &mut CheckpointManager,
        scripts: &[Script],
    ) -> Result<()> {
        let completed: HashSet<usize> = ckpt.audio_completed();
        let stage = AudioStage::new(self.config, self.tts_client);
        stage.run(scripts, &completed).await?;
        for s in scripts {
            ckpt.mark_audio_done(s.segment_index);
        }
        ckpt.save()?;
        Ok(())
    }
}
