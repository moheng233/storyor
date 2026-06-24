//! 提示词模板加载
//!
//! 提示词外置于 `prompts/` 目录，便于迭代调优。
//! 加载顺序：工作区 `prompts/` → 可执行文件同级 `prompts/` → 内置默认模板。

use std::path::{Path, PathBuf};

use crate::error::{Result, StoryorError};

/// 加载提示词模板
///
/// 查找顺序：
/// 1. 当前工作目录 `./prompts/<name>`
/// 2. 可执行文件所在目录 `./prompts/<name>`
/// 3. 内置默认模板（见 `default_template`）
pub fn load(name: &str) -> Result<String> {
    // 1. 工作目录
    let cwd_path = PathBuf::from("prompts").join(name);
    if cwd_path.exists() {
        return std::fs::read_to_string(&cwd_path).map_err(StoryorError::from);
    }
    // 2. 可执行文件同级
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let exe_path = dir.join("prompts").join(name);
        if exe_path.exists() {
            return std::fs::read_to_string(&exe_path).map_err(StoryorError::from);
        }
    }
    // 3. 内置默认
    default_template(name)
        .ok_or_else(|| StoryorError::Prompt(format!("提示词模板缺失: {name}")))
}

/// 内置默认模板
fn default_template(name: &str) -> Option<String> {
    match name {
        "story_teller.md" => Some(
            r#"你是一位经验丰富的评书艺人，擅长将小说改编为评书风格的朗读剧本。

## 你的任务
根据给定的章节原文，生成评书风格的剧本，包含：
1. **角色库**：本段涉及的所有角色档案（含音色设定 guidance，用于 TTS 合成）
2. **段落分组**：按情绪/场景转变将台词划分为若干段落（paragraphs），每段是一次 TTS 调用单位
3. **衔接话**：传递给下一段的剧情衔接说明（handoff）

## 风格要求
- 旁白用评书口吻："话说……""且说……""按下……不表"
- 台词保留角色性格，可适度口语化
- 段落划分依据情绪起伏、场景转换，而非机械按角色切换
- 每段台词行数不超过 {{max_paragraph_lines}} 行

## 输出格式
严格输出 JSON：
```json
{
  "characters": [{"name":"角色名","profile":"简介","scene":"场景","guidance":"TTS音色设定"}],
  "paragraphs": [{"index":0,"lines":[{"speaker":"旁白","content":"台词","tags":["情绪标签"]}]}],
  "handoff": "下一段剧情衔接说明"
}
```
"#.to_string(),
        ),
        "summary.md" => Some(
            r#"请阅读以下章节并生成 1-2 句话的剧情摘要。

## 章节标题
{{title}}

## 章节正文
{{content}}

## 要求
- 概括核心剧情推进与关键事件
- 1-2 句话，不超过 80 字
- 不要评价，只陈述剧情
"#.to_string(),
        ),
        "segment.md" => Some(
            r#"你是剧情结构分析师。请将以下章节摘要按剧情弧线切分为若干剧情段。

## 章节摘要
{{summaries}}

## 切分原则
- 每段应是一个相对完整的剧情弧线（起承转合）
- 段落边界应在剧情转折点，而非机械按章切分
- 每段可跨多章，也可仅含一章
- 段落序号从 0 开始连续递增

## 输出格式
严格输出 JSON：
```json
{
  "segments": [
    {"index":0,"chapter_start":0,"chapter_end":3,"summary":"本段剧情概述"}
  ]
}
```
"#.to_string(),
        ),
        "script.md" => Some(
            r#"请根据以下信息生成本剧情段的评书剧本。

## 本段剧情概述
{{segment_summary}}

## 上段衔接话
{{prev_handoff}}

## 当前角色库
{{characters}}

## 本段章节原文
{{chapter_text}}

## 要求
1. 生成评书风格剧本，旁白用评书口吻
2. 更新角色库：新增本段出现的角色，已有角色可更新音色设定
3. 按情绪/场景转变划分段落，每段不超过 {{max_paragraph_lines}} 行台词
4. 末尾给出 handoff：传递给下一段的剧情衔接说明

## 输出格式
严格输出 JSON：
```json
{
  "characters": [{"name":"角色名","profile":"简介","scene":"场景","guidance":"TTS音色设定"}],
  "paragraphs": [{"index":0,"lines":[{"speaker":"旁白","content":"台词","tags":["情绪标签"]}]}],
  "handoff": "下一段剧情衔接说明"
}
```
"#.to_string(),
        ),
        _ => None,
    }
}

/// prompts 目录路径（工作区）
pub fn prompts_dir() -> PathBuf {
    PathBuf::from("prompts")
}

/// 初始化工作区 prompts 目录（写入默认模板，若不存在）
pub fn init_default_prompts(dir: &Path) -> Result<()> {
    let prompts = dir.join("prompts");
    std::fs::create_dir_all(&prompts)?;
    for name in ["story_teller.md", "summary.md", "segment.md", "script.md"] {
        let path = prompts.join(name);
        if !path.exists()
            && let Some(content) = default_template(name)
        {
            std::fs::write(&path, content)?;
        }
    }
    Ok(())
}
