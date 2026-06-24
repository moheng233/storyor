# storyor 开发状态

## 当前阶段：项目架构搭建完成 ✅

## 最新变更：OpenAI 兼容 `json_object` 输出模式 ✅

- [x] 大模型请求不再使用 `json_schema`
- [x] 改为通过 `response_format = { type = "json_object" }` 请求 JSON 对象输出
- [x] `segment.md` / `script.md` 提示词补强：只输出 JSON 对象，不要附加解释或 Markdown 代码块
- [x] `backend = "OpenAI"` 改为在程序内走自定义 `OpenAICompatibleProvider`，底层固定请求 `/v1/chat/completions`

## 最新人工验证：小样本真实调用（run4）

- [x] 输入样本：`output/sample_1ch.txt`（约 1 万字）
- [x] 章节切分成功：1 章
- [x] 章节摘要成功，产物见 `output/run4/summaries.json`
- [x] 剧情段切分成功，产物见 `output/run4/segments.json`
- [ ] 剧本生成到达真实模型调用，但当前返回 JSON 结构与 `ScriptBody` 不完全匹配，报错：`invalid type: string "handoff", expected struct Paragraph`
- [ ] 因剧本 JSON 结构问题，首轮人工验证已确认 `chat/completions + json_object` 路径可用，但还需要进一步增强剧本阶段的输出约束或增加重试/修复逻辑

## 最新变更：收紧剧本提示词 ✅

- [x] 收紧 `prompts/story_teller.md` 的系统约束，明确顶层只能有 `characters`、`paragraphs`、`handoff`
- [x] 明确 `paragraphs` 必须是段落对象数组，禁止混入字符串或把 `handoff` 放入其中
- [x] 明确 `lines`/`tags` 的字段和类型要求
- [x] 增加“正确示例 / 错误示例”，直接约束当前已出现的 `"handoff"` 混入 `paragraphs` 问题
- [x] 未引入自动重试逻辑

## 最新变更：台词内容内联情绪/动作 ✅

- [x] `ScriptLine` 改为仅保留 `speaker` + `content`
- [x] 不再要求 LLM 输出 `tags` 字段
- [x] 情绪、动作、语速提示直接内联到 `content`，例如：`（紧张，深呼吸）呼……冷静，冷静。`
- [x] TTS 直接消费 `content`，不再额外拼接 `（情绪：...）`

## 已完成

### Phase 1: 基础设施
- [x] `src/error.rs` — 统一错误类型 `StoryorError`（thiserror，含 IO/LLM/Json/Regex/Base64/Http/Checkpoint/Config/Parse/Prompt 变体）
- [x] `src/config.rs` — `ModelConfig` ×3（small/large/tts）+ `AppConfig` 全局参数 + clap CLI（`--input/--config/--output/--resume/--force/--audio-format/--concurrency/--chapter-regex`），支持 TOML/JSON 配置文件加载
- [x] `src/script.rs` — 核心数据结构（`Chapter`/`ChapterSummary`/`PlotSegment`/`CharacterProfile`/`CharacterLibrary`/`ScriptLine`/`Paragraph`/`Script`/`AudioClip`/`SegmentList`/`ScriptBody`）+ serde + `StructuredOutputFormat` schema 构造函数（`script_schema`/`segment_schema`）

### Phase 2: 小说解析与章节摘要
- [x] `src/novel/chapter.rs` — 正则章节切分 `ChapterSplitter`，支持边界（无标题/单章/序言/空内容/自定义正则），含 5 个单元测试
- [x] `src/pipeline/summary.rs` — 章节摘要（小模型，`buffer_unordered` 并发，受 `max_concurrency` 限制）

### Phase 3: 剧情段切分与剧本生成
- [x] `src/pipeline/segment.rs` — 剧情段切分（大模型，`StructuredOutputFormat` 保证 JSON，含 `parse_json_response` 兼容 ```json 代码块）
- [x] `src/pipeline/script.rs` — 剧本生成（大模型，顺序，注入「上段 handoff + 当前角色库 + 本段原文」，落盘 script/handoff/characters）
- [x] `src/character.rs` — `CharacterLibrary::merge`/`merge_library`/`render_for_prompt`，含 3 个单元测试

### Phase 4: TTS 合成与音频输出
- [x] `src/tts/client.rs` — `TtsClient` 封装，**绕过 `ChatProvider` trait**，直接 HTTP 调用 chat 接口，反序列化原始 JSON 提取 `choices[0].message.audio.data` 做 base64 解码；多轮消息构造 `[assistant(guidance), user(text), ...]`
- [x] `src/audio.rs` — 音频落盘 `audio/segment_{i:04}/paragraph_{j:04}.{fmt}` + `manifest.json` 生成（支持段内续跑）

### Phase 5: 编排与断点续跑
- [x] `src/checkpoint.rs` — `CheckpointManager` 维护 `checkpoint.json`，校验 `novel_hash`/`config_hash`，阶段级 + 段粒度续跑，含 2 个单元测试
- [x] `src/pipeline/mod.rs` — `Pipeline::run()` 串联 chapters→summaries→segments→scripts→audio，每阶段完成后写 checkpoint
- [x] `src/main.rs` — CLI 入口，构建 3 个 provider（小/大/TTS，各自 `resilient()` + `validator()`）

### 提示词模板
- [x] `src/prompts.rs` — 模板加载器（工作区 → 可执行文件目录 → 内置默认）
- [x] `prompts/story_teller.md` — 说书人风格系统提示词
- [x] `prompts/summary.md` — 章节摘要提示词
- [x] `prompts/segment.md` — 剧情段切分提示词
- [x] `prompts/script.md` — 剧本生成提示词

## 验证结果
- `cargo build` ✅ 编译通过
- `cargo test` ✅ **24 个测试全部通过**（单元 10 + 集成 14）
  - 单元测试 10 个：`novel::chapter` 5 + `character` 3 + `checkpoint` 2
  - 集成测试 `pipeline_stages` 8 个：摘要生成/跳过、剧情段 JSON 解析/越界钳制/代码块、剧本生成/角色库累积/续跑跳过/prompt 构造
  - 集成测试 `full_pipeline` 6 个：checkpoint 阶段级标记/段粒度续跑/hash 变更重置/force 忽略/完整产物目录结构/续跑场景
- `cargo clippy --all-targets` ✅ 无实质警告（仅文档格式警告，中文注释被误判）

## 集成测试详情
- `tests/common/mod.rs` — mock `ChatProvider`/`ChatResponse`（按调用顺序消费预设响应，记录收到的 user 消息）+ 测试数据
- `tests/pipeline_stages.rs` — 各阶段独立测试
  - `summary_stage_produces_summaries`：摘要生成 + 落盘 + 排序
  - `summary_stage_skips_when_artifact_exists`：产物已存在时跳过
  - `segment_stage_parses_json`：剧情段 JSON 解析 + 落盘
  - `segment_stage_clamps_invalid_chapter_range`：越界章节范围钳制
  - `segment_stage_parses_json_code_block`：兼容 ```json 代码块包裹
  - `script_stage_generates_and_updates_library`：剧本生成 + 角色库累积 + handoff 落盘
  - `script_stage_skips_completed_segments`：已完成段跳过
  - `script_stage_prompt_includes_handoff_and_library`：prompt 含 handoff + 角色库 + 原文
- `tests/full_pipeline.rs` — 完整流水线 + checkpoint
  - `checkpoint_marks_stages_done`：阶段级完成标记持久化
  - `checkpoint_segment_granularity_resume`：段粒度续跑
  - `checkpoint_resets_on_hash_change`：hash 变更重置状态
  - `checkpoint_force_ignores_existing`：force 模式忽略 checkpoint
  - `full_pipeline_artifacts_layout`：完整产物目录结构验证
  - `script_stage_resume_from_checkpoint`：续跑场景（段 0 落盘加载，段 1 新生成）

## 待办（后续迭代）
- [x] 集成测试：用 mock LLM provider 跑通 summary→segment→script 三阶段 ✅
- [ ] 手动验证：真实小模型跑 1-3 章短篇，检查摘要/切分/剧本/角色库一致性
- [ ] 手动验证：真实 TTS 模型跑单段剧本，检查 base64 音频提取与输出文件可播放
- [ ] 断点续跑验证：中途 Ctrl-C 后重启，确认跳过已完成阶段
- [ ] TTS 底层通信：调研 `llm` crate 是否暴露底层 HTTP 客户端（当前已用独立 `reqwest` 实现）

## 模块结构
```
src/
  main.rs           CLI 入口 (clap)
  config.rs         三模型配置 + 全局参数 + CLI
  error.rs          错误类型 (thiserror)
  novel/
    mod.rs
    chapter.rs      正则章节切分
  pipeline/
    mod.rs          流水线编排 + 断点续跑
    summary.rs      章节摘要 (小模型, 并行)
    segment.rs      剧情段切分 (大模型)
    script.rs       剧本生成 (大模型, 顺序, JSON)
  character.rs      角色库管理 + 合并更新
  script.rs         剧本数据结构 + JSON schema
  prompts.rs        提示词模板加载
  tts/
    mod.rs
    client.rs       chat 接口 TTS 封装 + 音频提取
  audio.rs          音频落盘 + manifest 生成
  checkpoint.rs     产物落盘 + 续跑检查
prompts/
  story_teller.md
  summary.md
  segment.md
  script.md
```
