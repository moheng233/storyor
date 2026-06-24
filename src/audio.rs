//! 音频落盘 + manifest 生成
//!
//! 音频落盘 `<output>/audio/segment_{i:04}/paragraph_{j:04}.mp3`，记录 `AudioClip`。
//! 生成 `<output>/manifest.json`：列出所有段落音频路径、对应剧情段、时长。

use std::collections::HashSet;
use std::path::PathBuf;

use tracing::{info, warn};

use crate::checkpoint::write_artifact;
use crate::config::AppConfig;
use crate::error::Result;
use crate::script::{AudioClip, Script};
use crate::tts::client::TtsClient;

/// 音频合成阶段
pub struct AudioStage<'a> {
    config: &'a AppConfig,
    tts_client: &'a TtsClient,
}

impl<'a> AudioStage<'a> {
    pub fn new(config: &'a AppConfig, tts_client: &'a TtsClient) -> Self {
        Self { config, tts_client }
    }

    /// 对所有剧本合成音频
    pub async fn run(&self, scripts: &[Script], completed: &HashSet<usize>) -> Result<()> {
        let audio_dir = self.config.output_dir.join("audio");
        let format = self.tts_client.audio_format();

        let mut clips: Vec<AudioClip> = load_existing_manifest(&self.config.output_dir)?;

        for script in scripts {
            if completed.contains(&script.segment_index) {
                info!("剧情段 {} 音频已完成，跳过", script.segment_index);
                continue;
            }
            let seg_dir = audio_dir.join(format!("segment_{:04}", script.segment_index));
            std::fs::create_dir_all(&seg_dir)?;

            for paragraph in &script.paragraphs {
                let clip_path = seg_dir.join(format!("paragraph_{:04}.{}", paragraph.index, format));
                let rel_path = format!(
                    "audio/segment_{:04}/paragraph_{:04}.{}",
                    script.segment_index, paragraph.index, format
                );

                // 若文件已存在则跳过（段内续跑）
                if clip_path.exists() {
                    info!("段落音频已存在，跳过：{}", rel_path);
                    upsert_clip(&mut clips, script.segment_index, paragraph.index, rel_path);
                    continue;
                }

                info!(
                    "合成剧情段 {} 段落 {} 音频",
                    script.segment_index, paragraph.index
                );
                match self
                    .tts_client
                    .synthesize_paragraph(paragraph, &script.characters)
                    .await
                {
                    Ok(audio_bytes) => {
                        std::fs::write(&clip_path, &audio_bytes)?;
                        upsert_clip(
                            &mut clips,
                            script.segment_index,
                            paragraph.index,
                            rel_path,
                        );
                    }
                    Err(e) => {
                        warn!(
                            "剧情段 {} 段落 {} 合成失败：{e}",
                            script.segment_index, paragraph.index
                        );
                        return Err(e);
                    }
                }
            }
        }

        // 写入 manifest
        let manifest_path = self.config.output_dir.join("manifest.json");
        write_artifact(&manifest_path, &clips)?;
        info!("manifest 已生成：{:?}（{} 个片段）", manifest_path, clips.len());
        Ok(())
    }
}

/// 加载已有 manifest（续跑时累加）
fn load_existing_manifest(output_dir: &std::path::Path) -> Result<Vec<AudioClip>> {
    let path = output_dir.join("manifest.json");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content).unwrap_or_default())
    } else {
        Ok(Vec::new())
    }
}

/// 插入或更新 clip
fn upsert_clip(
    clips: &mut Vec<AudioClip>,
    segment_index: usize,
    paragraph_index: usize,
    audio_path: String,
) {
    if let Some(c) = clips.iter_mut().find(|c| {
        c.segment_index == segment_index && c.paragraph_index == paragraph_index
    }) {
        c.audio_path = audio_path;
    } else {
        clips.push(AudioClip {
            segment_index,
            paragraph_index,
            audio_path,
            duration_secs: None,
        });
    }
}

/// 计算音频文件路径（供外部使用）
pub fn clip_path(output_dir: &std::path::Path, segment_index: usize, paragraph_index: usize, format: &str) -> PathBuf {
    output_dir
        .join("audio")
        .join(format!("segment_{:04}", segment_index))
        .join(format!("paragraph_{:04}.{}", paragraph_index, format))
}
