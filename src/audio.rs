//! 音频落盘 + manifest 生成
//!
//! 音频落盘 `<output>/audio/segment_{i:04}/p{para:04}_l{line:04}.mp3`，记录 `AudioClip`。
//! 生成 `<output>/manifest.json`：列出所有台词音频路径、对应剧情段/段落/行、时长。
//! 每段台词合成完成后，用 symphonia 解码 + hound 编码为 segment.wav。

use std::collections::HashSet;
use std::path::PathBuf;

use hound::{SampleFormat, WavSpec, WavWriter};
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
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

    /// 对所有剧本合成音频（逐行合成，每次 TTS 调用只含一对 user/assistant）
    pub async fn run(&self, scripts: &[Script], completed: &HashSet<usize>) -> Result<()> {
        let audio_dir = self.config.output_dir.join("audio");
        let format = self.tts_client.audio_format();

        let mut clips: Vec<AudioClip> = load_existing_manifest(&self.config.output_dir)?;

        for script in scripts {
            if completed.contains(&script.segment_index) {
                info!("剧情段 {} 音频已完成，跳过", script.segment_index);
                // 确保已完成的 segment 也有合并文件（兼容旧版本输出）
                self.concat_segment_if_missing(&audio_dir, script, format)?;
                continue;
            }
            let seg_dir = audio_dir.join(format!("segment_{:04}", script.segment_index));
            std::fs::create_dir_all(&seg_dir)?;

            for paragraph in &script.paragraphs {
                for (line_idx, line) in paragraph.lines.iter().enumerate() {
                    let clip_name = format!(
                        "p{:04}_l{:04}.{}",
                        paragraph.index, line_idx, format
                    );
                    let clip_path = seg_dir.join(&clip_name);
                    let rel_path = format!(
                        "audio/segment_{:04}/{}",
                        script.segment_index, clip_name
                    );

                    // 若文件已存在则跳过（行级续跑）
                    if clip_path.exists() {
                        info!("台词音频已存在，跳过：{}", rel_path);
                        upsert_clip(
                            &mut clips,
                            script.segment_index,
                            paragraph.index,
                            line_idx,
                            rel_path,
                        );
                        continue;
                    }

                    info!(
                        "合成剧情段 {} 段落 {} 行 {} 音频（{}）",
                        script.segment_index, paragraph.index, line_idx, line.speaker
                    );
                    match self
                        .tts_client
                        .synthesize_line(&line.speaker, &line.content, &line.description, &script.characters)
                        .await
                    {
                        Ok(audio_bytes) => {
                            std::fs::write(&clip_path, &audio_bytes)?;
                            upsert_clip(
                                &mut clips,
                                script.segment_index,
                                paragraph.index,
                                line_idx,
                                rel_path,
                            );
                        }
                        Err(e) => {
                            warn!(
                                "剧情段 {} 段落 {} 行 {} 合成失败：{e}",
                                script.segment_index, paragraph.index, line_idx
                            );
                            return Err(e);
                        }
                    }
                }
            }

            // 将本段所有台词音频拼接成一个完整音频文件
            self.concat_segment(&seg_dir, script, format)?;
        }

        // 写入 manifest
        let manifest_path = self.config.output_dir.join("manifest.json");
        write_artifact(&manifest_path, &clips)?;
        info!("manifest 已生成：{:?}（{} 个片段）", manifest_path, clips.len());
        Ok(())
    }

    /// 将剧情段内所有台词音频按顺序解码为 PCM 并拼接为 WAV
    fn concat_segment(
        &self,
        seg_dir: &std::path::Path,
        script: &Script,
        format: &str,
    ) -> Result<()> {
        let segment_audio = seg_dir.join("segment.wav");

        // 收集所有存在的音频文件路径
        let mut clip_paths: Vec<PathBuf> = Vec::new();
        for paragraph in &script.paragraphs {
            for (line_idx, _) in paragraph.lines.iter().enumerate() {
                let clip_name = format!("p{:04}_l{:04}.{}", paragraph.index, line_idx, format);
                let clip_path = seg_dir.join(&clip_name);
                if clip_path.exists() {
                    clip_paths.push(clip_path);
                }
            }
        }

        if clip_paths.is_empty() {
            return Ok(());
        }

        // 解码第一个文件以获取音频参数
        let (sample_rate, channels) = Self::probe_audio(&clip_paths[0])?;

        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&segment_audio, spec)
            .map_err(|e| crate::error::StoryorError::Llm(format!("创建 WAV 文件失败: {e}")))?;

        let mut total_samples = 0u64;
        for clip_path in &clip_paths {
            let samples = Self::decode_to_pcm(clip_path)?;
            for s in &samples {
                let s_i16 = (*s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                writer
                    .write_sample(s_i16)
                    .map_err(|e| crate::error::StoryorError::Llm(format!("写入 WAV 失败: {e}")))?;
            }
            total_samples += samples.len() as u64;
        }

        writer
            .finalize()
            .map_err(|e| crate::error::StoryorError::Llm(format!("完成 WAV 写入失败: {e}")))?;

        let size = std::fs::metadata(&segment_audio).map(|m| m.len()).unwrap_or(0);
        info!(
            "剧情段 {} 合并音频已生成：{:?}（{} 样本, {} 字节）",
            script.segment_index,
            segment_audio,
            total_samples,
            size
        );
        Ok(())
    }

    /// 探测音频文件的采样率和声道数
    fn probe_audio(path: &std::path::Path) -> Result<(u32, u16)> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let hint = symphonia::core::formats::probe::Hint::new();
        let probed = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| crate::error::StoryorError::Llm(format!("音频探测失败: {e}")))?;

        let track = probed
            .default_track(TrackType::Audio)
            .ok_or_else(|| crate::error::StoryorError::Llm("无默认音轨".into()))?;

        let audio_params = extract_audio_params(&track.codec_params)?;
        let sample_rate = audio_params.sample_rate.unwrap_or(44100);
        let channels = audio_params
            .channels
            .map(|c| c.count() as u16)
            .unwrap_or(1)
            .max(1);

        Ok((sample_rate, channels.max(1)))
    }

    /// 解码单个音频文件为 f32 PCM 样本（单声道或立体声交错）
    fn decode_to_pcm(path: &std::path::Path) -> Result<Vec<f32>> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let hint = symphonia::core::formats::probe::Hint::new();
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| crate::error::StoryorError::Llm(format!("解码失败: {e}")))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| crate::error::StoryorError::Llm("无默认音轨".into()))?;
        let track_id = track.id;
        let audio_params = extract_audio_params(&track.codec_params)?;

        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(
                &audio_params,
                &symphonia::core::codecs::audio::AudioDecoderOptions::default(),
            )
            .map_err(|e| crate::error::StoryorError::Llm(format!("创建解码器失败: {e}")))?;

        let mut pcm: Vec<f32> = Vec::new();
        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => {
                    return Err(crate::error::StoryorError::Llm(format!(
                        "读取音频包失败: {e}"
                    )));
                }
            };

            if packet.track_id != track_id {
                continue;
            }

            let decoded = decoder
                .decode(&packet)
                .map_err(|e| crate::error::StoryorError::Llm(format!("解码音频帧失败: {e}")))?;

            // 转换为交错 f32
            match decoded {
                GenericAudioBufferRef::F32(buf) => {
                    let n_channels = buf.spec().channels().count();
                    for frame in 0..buf.frames() {
                        for ch in 0..n_channels {
                            pcm.push(buf.plane(ch).unwrap_or(&[0.0])[frame]);
                        }
                    }
                }
                GenericAudioBufferRef::S16(buf) => {
                    let n_channels = buf.spec().channels().count();
                    for frame in 0..buf.frames() {
                        for ch in 0..n_channels {
                            let sample = buf.plane(ch).unwrap_or(&[0])[frame] as f32 / i16::MAX as f32;
                            pcm.push(sample);
                        }
                    }
                }
                GenericAudioBufferRef::U8(buf) => {
                    let n_channels = buf.spec().channels().count();
                    for frame in 0..buf.frames() {
                        for ch in 0..n_channels {
                            let sample = (buf.plane(ch).unwrap_or(&[128])[frame] as f32 - 128.0) / 128.0;
                            pcm.push(sample);
                        }
                    }
                }
                GenericAudioBufferRef::S32(buf) => {
                    let n_channels = buf.spec().channels().count();
                    for frame in 0..buf.frames() {
                        for ch in 0..n_channels {
                            let sample = buf.plane(ch).unwrap_or(&[0])[frame] as f32 / i32::MAX as f32;
                            pcm.push(sample);
                        }
                    }
                }
                GenericAudioBufferRef::F64(buf) => {
                    let n_channels = buf.spec().channels().count();
                    for frame in 0..buf.frames() {
                        for ch in 0..n_channels {
                            let sample = buf.plane(ch).unwrap_or(&[0.0])[frame] as f32;
                            pcm.push(sample);
                        }
                    }
                }
                _ => {
                    return Err(crate::error::StoryorError::Llm(
                        "不支持的音频采样格式".into(),
                    ));
                }
            }
        }

        Ok(pcm)
    }

    /// 对已完成 segment，若合并文件不存在则补生成（兼容旧版本输出）
    fn concat_segment_if_missing(
        &self,
        audio_dir: &std::path::Path,
        script: &Script,
        format: &str,
    ) -> Result<()> {
        let seg_dir = audio_dir.join(format!("segment_{:04}", script.segment_index));
        let segment_audio = seg_dir.join("segment.wav");
        if !segment_audio.exists() {
            self.concat_segment(&seg_dir, script, format)?;
        }
        Ok(())
    }
}

/// 从 `Option<CodecParameters>` 中提取音频编解码参数
fn extract_audio_params(
    params: &Option<CodecParameters>,
) -> Result<symphonia::core::codecs::audio::AudioCodecParameters> {
    match params {
        Some(CodecParameters::Audio(audio)) => Ok(audio.clone()),
        _ => Err(crate::error::StoryorError::Llm(
            "无音频编解码参数".into(),
        )),
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
    line_index: usize,
    audio_path: String,
) {
    if let Some(c) = clips.iter_mut().find(|c| {
        c.segment_index == segment_index
            && c.paragraph_index == paragraph_index
            && c.line_index == line_index
    }) {
        c.audio_path = audio_path;
    } else {
        clips.push(AudioClip {
            segment_index,
            paragraph_index,
            line_index,
            audio_path,
            duration_secs: None,
        });
    }
}

/// 计算音频文件路径（供外部使用）
pub fn clip_path(
    output_dir: &std::path::Path,
    segment_index: usize,
    paragraph_index: usize,
    line_index: usize,
    format: &str,
) -> PathBuf {
    output_dir
        .join("audio")
        .join(format!("segment_{:04}", segment_index))
        .join(format!("p{:04}_l{:04}.{}", paragraph_index, line_index, format))
}
