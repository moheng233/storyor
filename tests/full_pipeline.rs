//! 集成测试：完整流水线 + checkpoint 续跑
//!
//! 验证 chapters → summaries → segments → scripts 全流程串联，
//! 以及 checkpoint 的阶段级跳过与段粒度续跑。

mod common;

use common::{
    mock_script_response_0, mock_script_response_1, mock_segment_response, mock_summary_responses,
    sample_novel, MockProvider,
};
use storyor::checkpoint::{read_artifact, CheckpointManager};
use storyor::config::{AppConfig, ModelConfig};
use storyor::script::{PlotSegment, Script};

fn test_config(output_dir: &std::path::Path) -> AppConfig {
    AppConfig {
        small_model: ModelConfig {
            backend: "OpenAI".to_string(),
            api_key: Some("test".to_string()),
            base_url: Some("http://localhost".to_string()),
            model: "test-small".to_string(),
        },
        large_model: ModelConfig {
            backend: "OpenAI".to_string(),
            api_key: Some("test".to_string()),
            base_url: Some("http://localhost".to_string()),
            model: "test-large".to_string(),
        },
        tts_model: ModelConfig {
            backend: "OpenAI".to_string(),
            api_key: Some("test".to_string()),
            base_url: Some("http://localhost".to_string()),
            model: "test-tts".to_string(),
        },
        chapter_regex: r"第[零一二三四五六七八九十百千0-9]{1,6}[章节回]".to_string(),
        max_concurrency: 2,
        output_dir: output_dir.to_path_buf(),
        max_paragraph_lines: 20,
        audio_format: "mp3".to_string(),
        tts_timeout_secs: 10,
    }
}

/// 验证 checkpoint 的阶段级跳过：当 chapters/summaries/segments 已完成时，
/// 重新加载 checkpoint 应能识别这些阶段为 done。
#[test]
fn checkpoint_marks_stages_done() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let novel = sample_novel();
    let novel_hash = storyor::checkpoint::sha256_hex(&novel);
    let cfg_hash = storyor::checkpoint::config_hash(&config);

    let mut ckpt = CheckpointManager::load(tmp.path(), false).unwrap();
    ckpt.validate(&novel_hash, &cfg_hash).unwrap();
    ckpt.mark_chapters_done();
    ckpt.mark_summaries_done();
    ckpt.mark_segments_done();
    ckpt.set_scripts_total(2);
    ckpt.set_audio_total(2);
    ckpt.save().unwrap();

    // 重新加载
    let reloaded = CheckpointManager::load(tmp.path(), false).unwrap();
    assert!(reloaded.chapters_done());
    assert!(reloaded.summaries_done());
    assert!(reloaded.segments_done());
    assert_eq!(reloaded.scripts_total(), 2);
    assert_eq!(reloaded.audio_total(), 2);
}

/// 验证 checkpoint 的段粒度续跑：标记 segment 0 已完成后，
/// 重新加载应能识别并跳过该段。
#[test]
fn checkpoint_segment_granularity_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let novel_hash = storyor::checkpoint::sha256_hex(&sample_novel());
    let cfg_hash = storyor::checkpoint::config_hash(&config);

    let mut ckpt = CheckpointManager::load(tmp.path(), false).unwrap();
    ckpt.validate(&novel_hash, &cfg_hash).unwrap();
    ckpt.set_scripts_total(2);
    ckpt.mark_script_done(0);
    ckpt.save().unwrap();

    let reloaded = CheckpointManager::load(tmp.path(), false).unwrap();
    let completed = reloaded.scripts_completed();
    assert!(completed.contains(&0), "段 0 应标记为已完成");
    assert!(!completed.contains(&1), "段 1 应未完成");
}

/// 验证 checkpoint hash 变更时重置状态
#[test]
fn checkpoint_resets_on_hash_change() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let novel_hash = storyor::checkpoint::sha256_hex(&sample_novel());
    let cfg_hash = storyor::checkpoint::config_hash(&config);

    let mut ckpt = CheckpointManager::load(tmp.path(), false).unwrap();
    ckpt.validate(&novel_hash, &cfg_hash).unwrap();
    ckpt.mark_chapters_done();
    ckpt.mark_summaries_done();
    ckpt.save().unwrap();

    // 用不同的 novel hash 重新加载
    let mut ckpt2 = CheckpointManager::load(tmp.path(), false).unwrap();
    let consistent = ckpt2
        .validate(&storyor::checkpoint::sha256_hex("完全不同的小说"), &cfg_hash)
        .unwrap();
    assert!(!consistent, "hash 变更应返回 false");
    // 状态应被重置
    assert!(!ckpt2.chapters_done());
    assert!(!ckpt2.summaries_done());
}

/// 验证 force 模式忽略 checkpoint
#[test]
fn checkpoint_force_ignores_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let novel_hash = storyor::checkpoint::sha256_hex(&sample_novel());
    let cfg_hash = storyor::checkpoint::config_hash(&config);

    // 先写入一个已完成的 checkpoint
    let mut ckpt = CheckpointManager::load(tmp.path(), false).unwrap();
    ckpt.validate(&novel_hash, &cfg_hash).unwrap();
    ckpt.mark_chapters_done();
    ckpt.save().unwrap();

    // force 模式加载应得到空状态
    let forced = CheckpointManager::load(tmp.path(), true).unwrap();
    assert!(!forced.chapters_done(), "force 模式应忽略已有 checkpoint");
}

/// 验证产物落盘的完整目录结构
#[tokio::test]
async fn full_pipeline_artifacts_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    // 阶段 1：章节切分
    let splitter = storyor::novel::ChapterSplitter::new(&config.chapter_regex).unwrap();
    let chapters = splitter.split(&sample_novel());
    assert_eq!(chapters.len(), 3);
    let chapters_path = tmp.path().join("chapters.json");
    storyor::checkpoint::write_artifact(&chapters_path, &chapters).unwrap();
    assert!(chapters_path.exists());

    // 阶段 2：摘要
    let small = MockProvider::new(mock_summary_responses());
    let summary_stage = storyor::pipeline::summary::SummaryStage::new(&config, &small);
    let summaries = summary_stage.run(&chapters).await.unwrap();
    assert!(tmp.path().join("summaries.json").exists());

    // 阶段 3：剧情段切分
    let large = MockProvider::new(vec![
        mock_segment_response(),
        mock_script_response_0(),
        mock_script_response_1(),
    ]);
    let segment_stage = storyor::pipeline::segment::SegmentStage::new(&config, &large);
    let segments = segment_stage.run(&summaries).await.unwrap();
    assert!(tmp.path().join("segments.json").exists());
    assert_eq!(segments.len(), 2);

    // 阶段 4：剧本生成
    let script_stage = storyor::pipeline::script::ScriptStage::new(&config, &large);
    let completed = std::collections::HashSet::new();
    let scripts = script_stage.run(&segments, &chapters, &completed).await.unwrap();
    assert_eq!(scripts.len(), 2);

    // 验证完整目录结构
    assert!(tmp.path().join("chapters.json").exists());
    assert!(tmp.path().join("summaries.json").exists());
    assert!(tmp.path().join("segments.json").exists());
    assert!(tmp.path().join("scripts").join("segment_0000.json").exists());
    assert!(tmp.path().join("scripts").join("segment_0000.handoff.txt").exists());
    assert!(tmp.path().join("scripts").join("segment_0001.json").exists());
    assert!(tmp.path().join("scripts").join("segment_0001.handoff.txt").exists());
    assert!(tmp.path().join("characters").join("final.json").exists());

    // 验证角色库累积一致性
    let library: storyor::script::CharacterLibrary =
        read_artifact(&tmp.path().join("characters").join("final.json"))
            .unwrap()
            .unwrap();
    assert_eq!(library.len(), 2);
    assert!(library.get("林风").is_some());
    assert!(library.get("白衣女子").is_some());

    // 验证剧本可被反序列化
    let s0: Script = read_artifact(&tmp.path().join("scripts").join("segment_0000.json"))
        .unwrap()
        .unwrap();
    assert_eq!(s0.segment_index, 0);
    assert_eq!(s0.paragraphs.len(), 2);
}

/// 验证续跑场景：剧本阶段部分完成后重启，应跳过已完成段
#[tokio::test]
async fn script_stage_resume_from_checkpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let splitter = storyor::novel::ChapterSplitter::new(&config.chapter_regex).unwrap();
    let chapters = splitter.split(&sample_novel());

    let segments = vec![
        PlotSegment {
            index: 0,
            chapter_start: 0,
            chapter_end: 1,
            summary: "段0".to_string(),
        },
        PlotSegment {
            index: 1,
            chapter_start: 2,
            chapter_end: 2,
            summary: "段1".to_string(),
        },
    ];

    // 第一次运行：生成段 0（completed 为空，不跳过）
    // mock 提供段 0 和段 1 的响应（段 1 也会被生成）
    let large = MockProvider::new(vec![
        mock_script_response_0(),
        mock_script_response_1(),
    ]);
    let script_stage = storyor::pipeline::script::ScriptStage::new(&config, &large);
    let completed = std::collections::HashSet::new();
    let scripts = script_stage.run(&segments, &chapters, &completed).await.unwrap();
    assert_eq!(scripts.len(), 2);
    assert_eq!(scripts[0].segment_index, 0);
    // 段 0 应已落盘
    assert!(tmp.path().join("scripts").join("segment_0000.json").exists());

    // 第二次运行：段 0 已落盘（completed 含 0），跳过段 0，只生成段 1
    let large2 = MockProvider::new(vec![mock_script_response_1()]);
    let script_stage2 = storyor::pipeline::script::ScriptStage::new(&config, &large2);
    let mut completed2 = std::collections::HashSet::new();
    completed2.insert(0);
    let scripts2 = script_stage2.run(&segments, &chapters, &completed2).await.unwrap();
    assert_eq!(scripts2.len(), 2);
    // 段 0 从落盘加载，段 1 新生成
    assert_eq!(scripts2[0].segment_index, 0);
    assert_eq!(scripts2[1].segment_index, 1);
    assert_eq!(scripts2[1].handoff, "二人踏上入京之路，前路未卜。");
    // mock 只应被调用 1 次（段 1）
    assert_eq!(large2.received_user_contents.lock().unwrap().len(), 1);
}
