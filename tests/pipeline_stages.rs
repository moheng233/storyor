//! 集成测试：pipeline 各阶段（summary → segment → script）
//!
//! 用 mock LLM provider 跑通三阶段，验证 JSON 解析、产物落盘、角色库一致性。

mod common;

use std::collections::HashSet;

use common::{
    mock_script_response_0, mock_script_response_1, mock_segment_response, mock_summary_responses,
    sample_novel, MockProvider,
};
use storyor::checkpoint::read_artifact;
use storyor::script::CharacterLibrary;
use storyor::config::{AppConfig, ModelConfig};
use storyor::novel::ChapterSplitter;
use storyor::pipeline::script::ScriptStage;
use storyor::pipeline::segment::SegmentStage;
use storyor::pipeline::summary::SummaryStage;
use storyor::script::{Chapter, ChapterSummary, PlotSegment, Script};

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 构造测试用配置（输出到临时目录）
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

/// 切分测试小说为章节
fn split_chapters(novel: &str) -> Vec<Chapter> {
    let splitter = ChapterSplitter::new(r"第[零一二三四五六七八九十百千0-9]{1,6}[章节回]").unwrap();
    splitter.split(novel)
}

// ---------------------------------------------------------------------------
// 阶段 1：章节摘要
// ---------------------------------------------------------------------------

#[tokio::test]
async fn summary_stage_produces_summaries() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let chapters = split_chapters(&sample_novel());
    assert_eq!(chapters.len(), 3, "应切分为 3 章");

    let provider = MockProvider::new(mock_summary_responses());
    let stage = SummaryStage::new(&config, &provider);
    let summaries = stage.run(&chapters).await.unwrap();

    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].chapter_index, 0);
    assert!(summaries[0].summary.contains("林风"));
    // 按章节序号排序
    assert_eq!(
        summaries.iter().map(|s| s.chapter_index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );

    // 验证落盘
    let path = tmp.path().join("summaries.json");
    let loaded: Vec<ChapterSummary> = read_artifact(&path).unwrap().unwrap();
    assert_eq!(loaded.len(), 3);

    // 验证 mock 收到了 user 消息（含章节内容）
    let received = provider.received_user_contents.lock().unwrap();
    assert_eq!(received.len(), 3, "应调用 3 次 chat");
    assert!(received[0].contains("初遇"));
}

#[tokio::test]
async fn summary_stage_skips_when_artifact_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let chapters = split_chapters(&sample_novel());

    // 预先写入 summaries.json
    let existing: Vec<ChapterSummary> = (0..3)
        .map(|i| ChapterSummary {
            chapter_index: i,
            summary: format!("预置摘要{i}"),
        })
        .collect();
    let path = tmp.path().join("summaries.json");
    std::fs::write(&path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

    // mock 队列为空，若被调用会报错
    let provider = MockProvider::new(vec![]);
    let stage = SummaryStage::new(&config, &provider);
    let summaries = stage.run(&chapters).await.unwrap();

    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].summary, "预置摘要0");
    // mock 不应被调用
    assert!(provider.received_user_contents.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// 阶段 2：剧情段切分
// ---------------------------------------------------------------------------

#[tokio::test]
async fn segment_stage_parses_json() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let summaries: Vec<ChapterSummary> = (0..3)
        .map(|i| ChapterSummary {
            chapter_index: i,
            summary: format!("摘要{i}"),
        })
        .collect();

    let provider = MockProvider::new(vec![mock_segment_response()]);
    let stage = SegmentStage::new(&config, &provider);
    let segments = stage.run(&summaries).await.unwrap();

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].index, 0);
    assert_eq!(segments[0].chapter_start, 0);
    assert_eq!(segments[0].chapter_end, 1);
    assert_eq!(segments[1].chapter_start, 2);
    assert_eq!(segments[1].chapter_end, 2);

    // 验证落盘
    let path = tmp.path().join("segments.json");
    let loaded: Vec<PlotSegment> = read_artifact(&path).unwrap().unwrap();
    assert_eq!(loaded.len(), 2);
}

#[tokio::test]
async fn segment_stage_clamps_invalid_chapter_range() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let summaries: Vec<ChapterSummary> = (0..2)
        .map(|i| ChapterSummary {
            chapter_index: i,
            summary: format!("摘要{i}"),
        })
        .collect();

    // 构造一个 chapter_end 越界的响应（只有 2 章，但 end=5）
    let bad_response = serde_json::json!({
        "segments": [
            {"index": 0, "chapter_start": 0, "chapter_end": 5, "summary": "越界段"}
        ]
    })
    .to_string();

    let provider = MockProvider::new(vec![bad_response]);
    let stage = SegmentStage::new(&config, &provider);
    let segments = stage.run(&summaries).await.unwrap();

    assert_eq!(segments.len(), 1);
    // chapter_end 应被钳制到 max_chapter-1 = 1
    assert_eq!(segments[0].chapter_end, 1);
}

#[tokio::test]
async fn segment_stage_parses_json_code_block() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let summaries: Vec<ChapterSummary> = vec![ChapterSummary {
        chapter_index: 0,
        summary: "摘要".to_string(),
    }];

    // 用 ```json 代码块包裹
    let wrapped = format!("```json\n{}\n```", mock_segment_response());
    let provider = MockProvider::new(vec![wrapped]);
    let stage = SegmentStage::new(&config, &provider);
    let segments = stage.run(&summaries).await.unwrap();
    assert_eq!(segments.len(), 2);
}

// ---------------------------------------------------------------------------
// 阶段 3：剧本生成
// ---------------------------------------------------------------------------

#[tokio::test]
async fn script_stage_generates_and_updates_library() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let chapters = split_chapters(&sample_novel());

    let segments = vec![
        PlotSegment {
            index: 0,
            chapter_start: 0,
            chapter_end: 1,
            summary: "林风初遇白衣女子，客栈遭袭".to_string(),
        },
        PlotSegment {
            index: 1,
            chapter_start: 2,
            chapter_end: 2,
            summary: "女子身份揭晓，林风护送入京".to_string(),
        },
    ];

    let provider = MockProvider::new(vec![
        mock_script_response_0(),
        mock_script_response_1(),
    ]);
    let stage = ScriptStage::new(&config, &provider);
    let completed = HashSet::new();
    let scripts = stage.run(&segments, &chapters, &completed).await.unwrap();

    assert_eq!(scripts.len(), 2);

    // 第一段：2 个段落
    assert_eq!(scripts[0].segment_index, 0);
    assert_eq!(scripts[0].paragraphs.len(), 2);
    assert_eq!(scripts[0].paragraphs[0].lines.len(), 2);
    assert_eq!(scripts[0].handoff, "二人结伴同行，夜宿客栈，暗流涌动。");

    // 第二段：1 个段落，handoff 更新
    assert_eq!(scripts[1].segment_index, 1);
    assert_eq!(scripts[1].paragraphs.len(), 1);
    assert_eq!(scripts[1].handoff, "二人踏上入京之路，前路未卜。");

    // 角色库应累积：第二段更新了白衣女子的 profile（加入"名萧云"）
    let chars_path = tmp.path().join("characters").join("final.json");
    let library: CharacterLibrary = read_artifact(&chars_path).unwrap().unwrap();
    assert_eq!(library.len(), 2, "应有 2 个角色");
    let baiyi = library.get("白衣女子").unwrap();
    assert!(
        baiyi.profile.contains("萧云"),
        "第二段应更新白衣女子 profile，实际: {}",
        baiyi.profile
    );

    // 验证剧本落盘
    let script_path = tmp.path().join("scripts").join("segment_0000.json");
    let loaded: Script = read_artifact(&script_path).unwrap().unwrap();
    assert_eq!(loaded.segment_index, 0);

    // 验证 handoff 纯文本落盘
    let handoff_path = tmp.path().join("scripts").join("segment_0000.handoff.txt");
    let handoff_text = std::fs::read_to_string(&handoff_path).unwrap();
    assert_eq!(handoff_text, scripts[0].handoff);
}

#[tokio::test]
async fn script_stage_skips_completed_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let chapters = split_chapters(&sample_novel());

    let segments = vec![PlotSegment {
        index: 0,
        chapter_start: 0,
        chapter_end: 0,
        summary: "段0".to_string(),
    }];

    // 预置 segment_0000.json，标记为已完成
    let pre_script = Script {
        segment_index: 0,
        characters: CharacterLibrary::new(),
        paragraphs: vec![],
        handoff: "预置handoff".to_string(),
    };
    let script_path = tmp.path().join("scripts").join("segment_0000.json");
    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(&script_path, serde_json::to_string_pretty(&pre_script).unwrap()).unwrap();

    // mock 队列为空，若被调用会报错
    let provider = MockProvider::new(vec![]);
    let stage = ScriptStage::new(&config, &provider);
    let mut completed = HashSet::new();
    completed.insert(0);
    let scripts = stage.run(&segments, &chapters, &completed).await.unwrap();

    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].handoff, "预置handoff");
    assert!(provider.received_user_contents.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// 阶段 4：prompt 构造验证
// ---------------------------------------------------------------------------

#[tokio::test]
async fn script_stage_prompt_includes_handoff_and_library() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let chapters = split_chapters(&sample_novel());

    let segment = PlotSegment {
        index: 0,
        chapter_start: 0,
        chapter_end: 0,
        summary: "测试段".to_string(),
    };

    let provider = MockProvider::new(vec![mock_script_response_0()]);
    let stage = ScriptStage::new(&config, &provider);

    let mut library = CharacterLibrary::new();
    // 注入一个已有角色，验证它会出现在 prompt 中
    use storyor::script::CharacterProfile;
    library.characters.insert(
        "林风".to_string(),
        CharacterProfile {
            name: "林风".to_string(),
            profile: "已有角色".to_string(),
            scene: "山道".to_string(),
            guidance: "青年男声".to_string(),
        },
    );

    let _ = stage
        .generate_segment(&segment, &chapters, &mut library, "上段衔接话XYZ")
        .await
        .unwrap();

    let received = provider.received_user_contents.lock().unwrap();
    assert_eq!(received.len(), 1);
    let prompt = &received[0];
    // prompt 应包含上段 handoff
    assert!(prompt.contains("上段衔接话XYZ"), "prompt 应包含 prev_handoff");
    // prompt 应包含已有角色库
    assert!(prompt.contains("林风"), "prompt 应包含角色库");
    // prompt 应包含本段章节原文
    assert!(prompt.contains("初遇"), "prompt 应包含章节原文");
}
