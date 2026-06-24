//! 剧本数据结构 + JSON schema 定义
//!
//! 定义流水线各阶段的核心数据结构，以及大模型生成剧本时
//! 所用的 `StructuredOutputFormat` schema 构造函数。

use std::collections::HashMap;

use llm::chat::StructuredOutputFormat;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// 基础数据结构
// ---------------------------------------------------------------------------

/// 章节切分结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    /// 章节序号（从 0 开始）
    pub index: usize,
    /// 章节标题
    pub title: String,
    /// 章节正文
    pub content: String,
}

/// 章节摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterSummary {
    /// 对应章节序号
    pub chapter_index: usize,
    /// 1-2 句摘要
    pub summary: String,
}

/// 剧情段（由大模型基于全部章节摘要切分）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotSegment {
    /// 段落序号（从 0 开始）
    pub index: usize,
    /// 起始章节序号（含）
    pub chapter_start: usize,
    /// 结束章节序号（含）
    pub chapter_end: usize,
    /// 本段剧情概述
    pub summary: String,
}

// ---------------------------------------------------------------------------
// 角色库
// ---------------------------------------------------------------------------

/// 单个角色档案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterProfile {
    /// 角色名
    pub name: String,
    /// 角色简介（身份、性格、关系）
    pub profile: String,
    /// 出现场景
    pub scene: String,
    /// TTS 音色设定（注入 assistant 消息）
    pub guidance: String,
}

/// 角色库
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterLibrary {
    /// 角色名 -> 档案
    pub characters: HashMap<String, CharacterProfile>,
}

// ---------------------------------------------------------------------------
// 剧本
// ---------------------------------------------------------------------------

/// 单句台词
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLine {
    /// 说话者（角色名或 "旁白"）
    pub speaker: String,
    /// 台词内容，需直接内联情绪/动作提示，例如：
    /// "（紧张，深呼吸）呼……冷静，冷静。"
    pub content: String,
    /// 导演模式描述：从角色/场景/指导三个维度刻画当前台词的演绎方式
    pub description: String,
}

/// 段落：一组连续台词，作为一次 TTS 调用的单位
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    /// 段落序号（在当前剧情段内从 0 开始）
    pub index: usize,
    /// 该段落包含的台词
    pub lines: Vec<ScriptLine>,
}

/// 剧本：一个剧情段对应的完整剧本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    /// 对应剧情段序号
    pub segment_index: usize,
    /// 本段角色库（更新后）
    pub characters: CharacterLibrary,
    /// 段落分组（由大模型直接输出）
    pub paragraphs: Vec<Paragraph>,
    /// 传递给下一段的剧情衔接说明
    pub handoff: String,
}

// ---------------------------------------------------------------------------
// 音频清单
// ---------------------------------------------------------------------------

/// 单个音频片段元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioClip {
    /// 对应剧情段序号
    pub segment_index: usize,
    /// 对应段落序号
    pub paragraph_index: usize,
    /// 对应行序号
    pub line_index: usize,
    /// 音频文件相对路径
    pub audio_path: String,
    /// 时长（秒），如可获取
    #[serde(default)]
    pub duration_secs: Option<f64>,
}

// ---------------------------------------------------------------------------
// StructuredOutputFormat schema 构造
// ---------------------------------------------------------------------------

/// 构造剧本生成的 JSON schema（用于 `LLMBuilder::schema()`）
///
/// 对应 `Script` 中由大模型直接输出的部分（不含 segment_index）：
/// ```json
/// {
///   "characters": [{"name","profile","scene","guidance"}],
///   "paragraphs": [{"index","lines":[{"speaker","content","description"}]}],
///   "handoff": "..."
/// }
/// ```
pub fn script_schema() -> StructuredOutputFormat {
    let line_schema = json!({
        "type": "object",
        "properties": {
            "speaker": {"type": "string", "description": "说话者角色名，旁白用\"旁白\""},
            "content": {"type": "string", "description": "完整台词文本，需直接内联情绪/动作提示，例如：\"（紧张，深呼吸）呼……冷静，冷静。\""},
            "description": {"type": "string", "description": "导演模式：从角色/场景/指导三维度刻画当前台词的演绎方式（100-300字）"}
        },
        "required": ["speaker", "content", "description"],
        "additionalProperties": false
    });

    let paragraph_schema = json!({
        "type": "object",
        "properties": {
            "index": {"type": "integer", "description": "段落序号，从0开始"},
            "lines": {
                "type": "array",
                "items": line_schema,
                "description": "该段落包含的连续台词"
            }
        },
        "required": ["index", "lines"],
        "additionalProperties": false
    });

    let character_schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "角色名"},
            "profile": {"type": "string", "description": "角色身份、性格、关系简介"},
            "scene": {"type": "string", "description": "出场场景"},
            "guidance": {"type": "string", "description": "TTS音色设定，用于assistant消息"}
        },
        "required": ["name", "profile", "scene", "guidance"],
        "additionalProperties": false
    });

    let schema: Value = json!({
        "type": "object",
        "properties": {
            "characters": {
                "type": "array",
                "items": character_schema,
                "description": "本段涉及的角色档案（含更新后的角色库）"
            },
            "paragraphs": {
                "type": "array",
                "items": paragraph_schema,
                "description": "按情绪/场景转变划分的段落分组"
            },
            "handoff": {
                "type": "string",
                "description": "传递给下一段的剧情衔接说明"
            }
        },
        "required": ["characters", "paragraphs", "handoff"],
        "additionalProperties": false
    });

    StructuredOutputFormat {
        name: "script".to_string(),
        description: Some("评书朗读剧本，含角色库、段落分组与衔接话".to_string()),
        schema: Some(schema),
        strict: Some(true),
    }
}

/// 构造剧情段切分的 JSON schema（用于 `LLMBuilder::schema()`）
///
/// 输出 `Vec<PlotSegment>` 的包装：
/// ```json
/// { "segments": [{"index","chapter_start","chapter_end","summary"}] }
/// ```
pub fn segment_schema() -> StructuredOutputFormat {
    let segment_schema = json!({
        "type": "object",
        "properties": {
            "index": {"type": "integer", "description": "段落序号，从0开始"},
            "chapter_start": {"type": "integer", "description": "起始章节序号（含）"},
            "chapter_end": {"type": "integer", "description": "结束章节序号（含）"},
            "summary": {"type": "string", "description": "本段剧情概述"}
        },
        "required": ["index", "chapter_start", "chapter_end", "summary"],
        "additionalProperties": false
    });

    let schema: Value = json!({
        "type": "object",
        "properties": {
            "segments": {
                "type": "array",
                "items": segment_schema,
                "description": "剧情段切分结果"
            }
        },
        "required": ["segments"],
        "additionalProperties": false
    });

    StructuredOutputFormat {
        name: "segments".to_string(),
        description: Some("剧情段切分结果".to_string()),
        schema: Some(schema),
        strict: Some(true),
    }
}

/// 大模型剧情段切分的响应包装
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentList {
    pub segments: Vec<PlotSegment>,
}

/// 大模型剧本生成的响应包装（不含 segment_index）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptBody {
    pub characters: Vec<CharacterProfile>,
    pub paragraphs: Vec<Paragraph>,
    pub handoff: String,
}

impl ScriptBody {
    /// 转换为完整 `Script`，并合并角色库
    pub fn into_script(
        self,
        segment_index: usize,
        library: &mut CharacterLibrary,
    ) -> Script {
        for ch in &self.characters {
            library.characters.insert(ch.name.clone(), ch.clone());
        }
        Script {
            segment_index,
            characters: library.clone(),
            paragraphs: self.paragraphs,
            handoff: self.handoff,
        }
    }
}
