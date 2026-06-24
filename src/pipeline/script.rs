//! 剧本生成（大模型，顺序，JSON）
//!
//! 顺序遍历剧情段。每段构造 prompt = 系统提示（说书人风格模板）
//! + 上段 handoff + 当前角色库 + 本段原文。用 `schema()` + `validator()`
//! 强制 JSON 输出并校验。解析为 `Script`，更新角色库（合并新增/更新已有），
//! 落盘 `<output>/scripts/segment_{i:04}.json` + `<output>/scripts/segment_{i:04}.handoff.txt`，
//! 并每段后刷新 `<output>/characters/final.json`。

use std::path::{Path, PathBuf};

use llm::chat::{ChatMessage, ChatProvider};
use tracing::info;

use crate::checkpoint::write_artifact;
use crate::config::AppConfig;
use crate::script::CharacterLibrary;
use crate::error::{Result, StoryorError};
use crate::script::{Chapter, PlotSegment, Script, ScriptBody};

/// 剧本生成阶段
pub struct ScriptStage<'a> {
    config: &'a AppConfig,
    large_model: &'a dyn ChatProvider,
}

impl<'a> ScriptStage<'a> {
    pub fn new(config: &'a AppConfig, large_model: &'a dyn ChatProvider) -> Self {
        Self { config, large_model }
    }

    /// 生成单个剧情段的剧本
    ///
    /// - `segment`: 当前剧情段
    /// - `chapters`: 全部章节（用于提取本段原文）
    /// - `library`: 当前角色库（会被更新）
    /// - `prev_handoff`: 上段衔接话（首段为空）
    pub async fn generate_segment(
        &self,
        segment: &PlotSegment,
        chapters: &[Chapter],
        library: &mut CharacterLibrary,
        prev_handoff: &str,
    ) -> Result<Script> {
        let story_teller = crate::prompts::load("story_teller.md")?;
        let prompt_template = crate::prompts::load("script.md")?;

        // 提取本段原文
        let chapter_text = chapters
            .iter()
            .filter(|c| c.index >= segment.chapter_start && c.index <= segment.chapter_end)
            .map(|c| format!("## {} \n{}", c.title, c.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = prompt_template
            .replace("{{segment_summary}}", &segment.summary)
            .replace("{{prev_handoff}}", prev_handoff)
            .replace("{{characters}}", &library.render_for_prompt())
            .replace("{{chapter_text}}", &chapter_text)
            .replace("{{max_paragraph_lines}}", &self.config.max_paragraph_lines.to_string());

        let messages = vec![
            ChatMessage::assistant().content(story_teller).build(),
            ChatMessage::user().content(prompt).build(),
        ];

        info!(
            "生成剧情段 {} 剧本（章节 {}-{}）",
            segment.index, segment.chapter_start, segment.chapter_end
        );

        let resp = self
            .large_model
            .chat(&messages)
            .await
            .map_err(|e| StoryorError::Llm(e.to_string()))?;

        let text = resp
            .text()
            .ok_or_else(|| StoryorError::Llm("剧本生成响应无文本".into()))?;

        let body: ScriptBody = super::segment::parse_json_response(&text)?;
        let script = body.into_script(segment.index, library);

        // 落盘
        let scripts_dir = self.config.output_dir.join("scripts");
        let script_path = scripts_dir.join(format!("segment_{:04}.json", segment.index));
        let handoff_path = scripts_dir.join(format!("segment_{:04}.handoff.txt", segment.index));
        write_artifact(&script_path, &script)?;
        std::fs::write(&handoff_path, &script.handoff)?;

        // 刷新角色库
        let chars_dir = self.config.output_dir.join("characters");
        let chars_path = chars_dir.join("final.json");
        write_artifact(&chars_path, library)?;

        info!(
            "剧情段 {} 剧本完成，{} 段落，已落盘",
            segment.index,
            script.paragraphs.len()
        );
        Ok(script)
    }

    /// 顺序生成所有剧情段剧本
    pub async fn run(
        &self,
        segments: &[PlotSegment],
        chapters: &[Chapter],
        completed: &std::collections::HashSet<usize>,
    ) -> Result<Vec<Script>> {
        let mut library = load_or_init_library(&self.config.output_dir)?;
        let mut scripts = Vec::with_capacity(segments.len());
        let mut prev_handoff = String::new();

        for segment in segments {
            if completed.contains(&segment.index) {
                // 续跑：从落盘文件加载
                let path = self
                    .config
                    .output_dir
                    .join("scripts")
                    .join(format!("segment_{:04}.json", segment.index));
                if let Ok(Some(script)) =
                    crate::checkpoint::read_artifact::<Script>(&path)
                {
                    info!("剧情段 {} 剧本已存在，跳过", segment.index);
                    // 合并角色库以保持后续一致性
                    library.merge_library(&script.characters);
                    prev_handoff = script.handoff.clone();
                    scripts.push(script);
                }
                continue;
            }
            let script = self
                .generate_segment(segment, chapters, &mut library, &prev_handoff)
                .await?;
            prev_handoff = script.handoff.clone();
            scripts.push(script);
        }
        Ok(scripts)
    }
}

/// 加载已有角色库或初始化空库
fn load_or_init_library(output_dir: &Path) -> Result<CharacterLibrary> {
    let path: PathBuf = output_dir.join("characters").join("final.json");
    if let Ok(Some(lib)) = crate::checkpoint::read_artifact::<CharacterLibrary>(&path) {
        return Ok(lib);
    }
    Ok(CharacterLibrary::new())
}
