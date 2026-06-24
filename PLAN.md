# Plan: 小说评书朗读生成器 (storyor)

## TL;DR
基于 `llm` crate 构建一个三阶段流水线：小模型逐章摘要 → 大模型切分剧情段并生成 JSON 剧本（含角色库+衔接话）→ TTS 模型按段落合成音频。支持断点续跑、分段音频输出与清单管理。

## 用户决策
- 章节切分：正则匹配章节标题
- TTS 音色：chat 接口传消息（assistant=角色音色设定，user=台词文本），音频以 base64 形式返回在 `choices[0].message.audio.data`
- 角色一致性：独立角色库，顺序生成，每段携带「上段衔接话 + 上段角色库」
- 音频输出：分段音频 + manifest 清单 + 断点续跑
- 分段策略：由大模型在生成剧本时同时输出段落分组（而非后处理）
- 提示词模板：外置于配置文件中（`prompts/` 目录）

## 库能力要点 (llm 1.3.8)
- `LLMBuilder::new().backend(LLMBackend).api_key().base_url().model().system().schema(StructuredOutputFormat).voice().memory().resilient().build()` -> `Box<dyn LLMProvider>`
- `LLMBackend`: OpenAI/Anthropic/Ollama/DeepSeek/XAI/Google/Groq/ElevenLabs/Mistral/OpenRouter/HuggingFace 等，实现 `FromStr`
- `ChatProvider::chat(&[ChatMessage])` -> `Box<dyn ChatResponse>`; `ChatResponse::text()` -> `Option<String>`
- `TextToSpeechProvider::speech(&str)` -> `Vec<u8>`
- `ChatMessage::user()`/`assistant()` -> `ChatMessageBuilder`
- `StructuredOutputFormat { name, description, schema: Option<Value>, strict: Option<bool> }` 用于 JSON 结构化输出
- `LLMBuilder::validator()` + `validator_attempts()` 可校验输出并重试
- `LLMBuilder::resilient()` + `resilient_attempts()` + `resilient_backoff()` 自动重试退避
- `LLMBuilder::extra_body(Serialize)` 可注入 provider 特有参数

## 数据结构设计
- `Chapter { index, title, content }`
- `ChapterSummary { chapter_index, summary }`
- `PlotSegment { index, chapter_range, summary }`
- `CharacterProfile { name, profile, scene, guidance }` — guidance 即 TTS 音色设定
- `CharacterLibrary { characters: HashMap<String, CharacterProfile> }`
- `ScriptLine { speaker, content, tags: Vec<String> }`
- `Paragraph { index, lines: Vec<ScriptLine> }` — 一组连续台词，作为一次 TTS 调用的单位
- `Script { segment_index, characters: CharacterLibrary, paragraphs: Vec<Paragraph>, handoff: String }`
- `AudioClip { segment_index, paragraph_index, audio_path, duration_secs: Option<f64> }`

## 剧本 JSON 输出格式 (StructuredOutputFormat schema)
由大模型在生成剧本时**同时输出段落分组**（`paragraphs`），不再后处理切分。
```json
{
  "characters": [{"name","profile","scene","guidance"}],
  "paragraphs": [
    {
      "index": 0,
      "lines": [{"speaker":"旁白","content":"...","tags":[]}, {"speaker":"角色A","content":"...","tags":["怅然"]}]
    }
  ],
  "handoff": "传递给下一段的剧情衔接说明"
}
```

## TTS 多轮对话格式 (每段落一次调用)
对每个 `Paragraph`（含多句台词），构造多轮 chat 消息：
```json
// 请求消息（由 tts/client.rs 构造）
messages = [
  ChatMessage::assistant().content(角色1.guidance),
  ChatMessage::user().content(台词1),
  ChatMessage::assistant().content(角色2.guidance),
  ChatMessage::user().content(台词2),
  ...
]
```

### TTS 响应格式（实际 API 返回）
音频以 **base64** 形式嵌套在标准 chat completion 的扩展字段中，不走 `ChatResponse::text()`：
```json
{
  "choices": [{
    "message": {
      "content": "",
      "role": "assistant",
      "audio": {
        "data": "<base64 encoded audio>",
        "id": "...",
        "expires_at": null,
        "transcript": null
      }
    }
  }]
}
```

### 音频提取策略
`llm` crate 的 `ChatResponse` trait 仅暴露 `text()` 和 `tool_calls()`，不包含 `audio` 扩展字段。
因此 `TtsClient` 需直接反序列化原始响应 JSON 提取 `choices[0].message.audio.data`，不走 trait 抽象。
实现为 `AudioExtractor::Base64` 策略（后续可扩展其他格式）。

## 模块结构
```
src/
  main.rs           CLI 入口 (clap)
  config.rs         三模型配置 + 全局参数
  error.rs          错误类型 (thiserror)
  novel/chapter.rs  正则章节切分
  pipeline/
    mod.rs          流水线编排 + 断点续跑
    summary.rs      章节摘要 (小模型, 并行)
    segment.rs      剧情段切分 (大模型)
    script.rs       剧本生成 (大模型, 顺序, JSON)
  character.rs      角色库管理 + 合并更新
  script.rs         剧本数据结构 + JSON schema 定义
  prompts/             提示词模板目录（外置配置）
    story_teller.md    说书人风格系统提示词
    summary.md         章节摘要提示词
    segment.md         剧情段切分提示词
  tts/
    client.rs          chat 接口 TTS 封装 + 音频提取策略
  audio.rs          音频落盘 + manifest 生成
  checkpoint.rs     产物落盘 + 续跑检查
```

## 产物落盘目录结构（固定）
根目录 `<output_dir>/`（默认 `./output`，CLI `--output` 配置）。所有中间产物与最终音频均落盘于此，命名用零填充（4 位）保证排序。`checkpoint.json` 驱动断点续跑。

```
<output_dir>/
├── chapters.json                 # 章节切分结果 Vec<Chapter>（index/title/content）
├── summaries.json                # 章节摘要 Vec<ChapterSummary>
├── segments.json                 # 剧情段切分 Vec<PlotSegment>
├── characters/
│   └── final.json                # 最终累积角色库 CharacterLibrary
├── scripts/
│   ├── segment_0001.json         # 完整剧本 Script（characters/lines/handoff）
│   ├── segment_0001.handoff.txt  # 衔接话纯文本（便于人工审阅衔接逻辑）
│   ├── segment_0002.json
│   └── segment_0002.handoff.txt
├── audio/
│   ├── segment_0001/
│   │   ├── paragraph_0001.mp3    # 单段落音频（一段多轮对话合成结果）
│   │   └── paragraph_0002.mp3
│   └── segment_0002/
│       └── paragraph_0001.mp3
├── manifest.json                 # 音频清单：所有 AudioClip 汇总（路径/剧情段/角色/时长）
└── checkpoint.json                # 断点续跑状态机
```

### checkpoint.json 结构
```json
{
  "novel_hash": "<sha256 of input novel>",
  "config_hash": "<hash of relevant config>",
  "stages": {
    "chapters":  "done",
    "summaries": "done",
    "segments":  "done",
    "scripts":   { "completed": [0, 1], "total": 5 },
    "audio":     { "completed": [0],    "total": 5 }
  }
}
```
- `novel_hash`/`config_hash` 变更时提示全量重跑（旧产物失效）
- 阶段级（chapters/summaries/segments）整体完成标记；段级（scripts/audio）记录已完成索引集合，支持段粒度续跑

## Steps

### Phase 1: 基础设施
1. 定义 `config.rs`：`ModelConfig{backend,api_key,base_url,model}` ×3（small/large/tts）+ 全局参数（正则、并发数、输出目录、段落长度上限）。用 clap derive 暴露 CLI。
2. 定义 `error.rs`：用 thiserror 统一 `StoryorError`（IO/LLM/Parse/Checkpoint 变体）。
3. 定义 `script.rs`：上述数据结构 + `serde` 序列化/反序列化 + `StructuredOutputFormat` schema 构造函数。

### Phase 2: 小说解析与章节摘要
4. `novel/chapter.rs`：正则切分章节，返回 `Vec<Chapter>`。正则可配置（默认 `第.{1,6}章` / `Chapter \d+`）。结果落盘 `<output>/chapters.json`。
5. `pipeline/summary.rs`：用小模型对每章生成 1-2 句摘要。`buffer_unordered` 并发（受 `max_concurrency` 限制）。结果落盘 `<output>/summaries.json`。

### Phase 3: 剧情段切分与剧本生成
6. `pipeline/segment.rs`：把所有章节摘要交给大模型，输出 `Vec<PlotSegment>`（含起止章节、剧情概述）。用 `StructuredOutputFormat` 保证 JSON。落盘 `<output>/segments.json`。
7. `pipeline/script.rs`：顺序遍历剧情段。每段构造 prompt = 系统提示（说书人风格模板）+ 上段 handoff + 当前角色库 + 本段原文。用 `schema()` + `validator()` 强制 JSON 输出并校验。解析为 `Script`，更新角色库（合并新增/更新已有），落盘 `<output>/scripts/segment_{i:04}.json` + `<output>/scripts/segment_{i:04}.handoff.txt`，并每段后刷新 `<output>/characters/final.json`。
8. `character.rs`：`CharacterLibrary::merge(new_chars)` 合并策略——新角色加入、已有角色按段更新（保留最新 guidance）。

### Phase 4: TTS 合成与音频输出
9. `tts/client.rs`：`TtsClient` 封装。`synthesize_paragraph(paragraph, library)` 对每个 `Paragraph` 构造多轮 chat 消息 `[assistant(guidance), user(text), ...]`。**不走 `ChatProvider::chat()` 的 trait 抽象**——直接调用底层 HTTP 客户端并反序列化原始 JSON，提取 `choices[0].message.audio.data` 做 base64 解码为 `Vec<u8>`。音频格式由 CLI `--audio-format` 指定（默认 mp3）。
10. `audio.rs`：音频落盘 `<output>/audio/segment_{i:04}/paragraph_{j:04}.mp3`，记录 `AudioClip`。
11. 生成 `<output>/manifest.json`：列出所有段落音频路径、对应剧情段、角色、时长（如可获取）。

### Phase 5: 编排与断点续跑
12. `checkpoint.rs`：维护 `<output>/checkpoint.json`（见目录结构小节）。启动时读取，校验 `novel_hash`/`config_hash`，按阶段+段索引跳过已完成产物（chapters→summaries→segments→scripts→audio）。
13. `pipeline/mod.rs`：`Pipeline::run()` 串联全流程，每阶段/每段完成后写 checkpoint。支持 `--resume` 从最近断点继续，`--force` 忽略 checkpoint 全量重跑。
14. `main.rs`：解析 CLI → 构建 3 个 `Box<dyn LLMProvider>`（小/大/TTS，各自 `resilient()` + `validator()`）→ 运行 pipeline。

## Relevant files
- `Cargo.toml` — 已有 llm/clap/tokio/serde/serde_json/thiserror/tracing，可能需加 `regex`（章节切分）、`futures`（并发流，或用 tokio Stream）
- `src/main.rs` — 当前仅 hello world，将改为 CLI 入口
- `llm::builder::LLMBuilder` — 构建 provider，关键方法 `backend/api_key/base_url/model/system/schema/voice/resilient/validator/extra_body/build`
- `llm::chat::{ChatMessage, ChatProvider, ChatResponse, StructuredOutputFormat}` — 对话与结构化输出
- `llm::tts::TextToSpeechProvider` — `speech(&str)->Vec<u8>`（TTS 走 chat 接口时可能不用此 trait，而用 ChatProvider）

## Verification
1. `cargo build` 编译通过，`cargo clippy` 无警告
2. 单元测试：`novel/chapter.rs` 正则切分（含边界：无章节标题、单章、空内容）
3. 单元测试：`character.rs` 角色库合并（新增、更新、冲突处理）
4. 集成测试：用 mock LLM provider（或录制响应）跑通 summary→segment→script 三阶段，验证 JSON 解析（含 `paragraphs` 结构）
5. 手动验证：用真实小模型跑 1-3 章短篇，检查摘要质量、剧情段切分合理性、剧本格式、角色库一致性
6. 手动验证：用真实 TTS 模型跑单段剧本，检查 base64 音频提取与输出文件可播放
7. 断点续跑验证：中途 Ctrl-C 后重启，确认跳过已完成阶段

## Decisions
- TTS 走 chat 接口而非 `speech()`：因用户的 TTS 模型以 chat 多轮消息接收音色设定+台词，音频以 base64 返回在 `choices[0].message.audio.data`
- TTS 音频提取绕过 `ChatResponse` trait，直接反序列化原始 JSON 取 `audio.data` 字段
- 角色库顺序传递：每段生成时注入「上段 handoff + 当前角色库」，模型输出更新后的角色库 + 新 handoff，天然保持一致性
- 剧本生成用 `StructuredOutputFormat` + `validator()` 双保险保证 JSON 可解析
- 大模型在生成剧本时同时输出段落分组（`paragraphs`），不再后处理。prompt 中需指导按情绪/场景转变划分
- 提示词模板外置于 `prompts/` 目录，便于迭代调优
- 中间产物全部落盘 JSON + checkpoint.json，支持段粒度断点续跑

## Further Considerations
1. ~~TTS 响应音频提取策略~~ → 已确定：base64 编码在 `choices[0].message.audio.data`，直接反序列化原始 JSON
2. ~~段落分组粒度~~ → 已确定：由大模型在生成剧本时输出 `paragraphs`，不再后处理
3. 大模型分段质量：需在剧本 prompt 中明确指导模型按"情绪/场景转变"划分段落，而非机械按角色切换。建议在 `prompts/story_teller.md` 中给出分段示例
4. TTS 底层通信：`llm` crate 的 `ChatProvider` trait 无法暴露 `audio` 扩展字段，需绕过 trait 直接调用。需调研 `llm` crate 是否暴露底层 HTTP 客户端或需自行用 `reqwest` 构造请求
