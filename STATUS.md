# storyor 开发状态

## 当前阶段：项目架构搭建完成 ✅

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
- `cargo test` ✅ 10 个单元测试全部通过
  - `novel::chapter` 5 个（多章/序言/无标题/空内容/英文正则）
  - `character` 3 个（新增/更新/合并）
  - `checkpoint` 2 个（段状态标记/SHA256 稳定性）
- `cargo clippy` ✅ 无实质警告（仅 dead_code 警告，均为后续阶段预留的公共 API）

## 待办（后续迭代）
- [ ] 集成测试：用 mock LLM provider 跑通 summary→segment→script 三阶段
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
