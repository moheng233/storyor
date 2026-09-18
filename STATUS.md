# storyor 开发状态

## 当前阶段：v2 交互式工作流 + Web UI 架构升级进行中

整体规划见 `PLAN.md`。下表按 Phase A–E 与附加子计划汇总当前完成度，已完成项已从 PLAN 中清除，未完成项保留在 PLAN 中。

### ✅ 已完成

#### Phase A：后端基础设施
- 依赖：`Cargo.toml` 引入 `axum`/`tower`/`tower-http`/`uuid`/`axfetchum`(含 `axum` feature)/`ts-rs`(含 `chrono-impl` feature)
- 错误类型：`src/error.rs` 的 `StoryorError` 实现 `axum::response::IntoResponse`，统一返回 `{"error": msg}` JSON（项目未找到→404，其余→500）
- 配置扩展：`src/config.rs` 新增 `ServerConfig`(host/port)、`WorkspaceConfig`(dir)、`TimingConfig`、`SoundsConfig`；CLI 简化为单一 `server` 模式（移除 v1 pipeline 子命令）
- 项目管理：`src/project.rs` — `ProjectManager` 实现按 `workspace_dir/<id>/` 分目录的 CRUD（list/create/get/delete），`ProjectMeta`/`ProjectListItem`/`ProjectPhase`/`CreateProjectRequest` 标注 `#[derive(TS)]`
- server 模块：
  - `src/server/mod.rs` — `run_server(config)` 启动 axum，`build_router` 组装路由 + CORS + 静态文件 fallback
  - `src/server/state.rs` — `AppState`（config + ProjectManager）
  - `src/server/types.rs` — `HealthResponse`/`ErrorResponse`
  - `src/server/routes/mod.rs` — `axfetchum::ApiRouter` builder 模式，一次产出 axum `Router` + `RouteCollection` 元数据；含 projects CRUD、健康检查、静态文件服务
  - `src/server/events.rs` — 进度事件类型骨架（`ProgressEvent` 枚举）
- 前端 TS 绑定导出：`tests/export_bindings.rs` 用 `ts_rs::TS::export_all(&Config)` 显式导出类型 + `axfetchum::generate_to_file` 生成 API 客户端，产物在 `frontend/src/bindings/`
- CLI 入口：`src/main.rs` 重写为 `server` 子命令入口

#### 子计划：用 reqwest 替换 `llm` crate
完全移除 `llm` crate，改为基于 `reqwest` 直连 OpenAI 兼容 `/v1/chat/completions` 客户端：

- 新增 `src/llm/` 模块，自包含 OpenAI 兼容客户端
  - `src/llm/mod.rs` — 模块入口，定义 `ChatClient` trait（三方法：`chat` / `chat_with_schema` / `chat_stream`）与 `TtsClient` trait（`chat_audio`），导出 `ChatMessage`/`ChatResponse`/`Choice`/`ResponseFormat`/`JsonSchemaFormat`/`Role` 等；`ChatDeltaStream<'a> = Pin<Box<dyn Stream<Item = Result<String>> + Send + 'a>>`
  - `src/llm/types.rs` — 手动 serde 结构：`ChatCompletionRequest`（含 `model`/`messages`/`response_format`/`audio`/`max_tokens`/`temperature`/`stream`）、`ResponseFormat::{Text,JsonObject,JsonSchema}`、`JsonSchemaFormat`、`ChatResponse`、`Choice`、`ResponseMessage`（含 `audio` 字段）、`AudioConfig`/`AudioData`、`OpenAiErrorBody`
  - `src/llm/client.rs` — `OpenAiClient`（`new` / `with_options`）同时实现 `ChatClient` 与 `TtsClient`；流式 `chat_stream` 通过 `async_stream` + `bytes_stream()` 解析 SSE；`ChatCompletionRequest.stream` 字段由 `build_stream_request` 显式设置，不再手动注入 JSON
  - `src/llm/error.rs` — `OpenAiError` 枚举严格类型化错误（`RequestFailed`/`BadStatus`/`Deserialize`/`SseParse`/`NoText`/`NoAudioData`），通过 `From<OpenAiError> for StoryorError` 自动上转
- 配置简化：移除 `llm::builder::LLMBackend` 与 `ModelConfig::parse_backend()`，新增 `backend_label()`（仅展示）；`backend` 字段保留为字符串，所有服务按 OpenAI 兼容接口处理，`base_url` 决定实际服务商
- 流水线接入新 trait：
  - `Pipeline`/`SummaryStage`/`SegmentStage`/`ScriptStage` 的 `small_model`/`large_model` 从 `&dyn ChatProvider` 改为 `&dyn ChatClient`
  - summary 用普通文本对话；segment/script 调用 `chat_with_schema(messages, Some(&segment_schema()/script_schema()))` 走 `json_schema` 严格模式
  - `script_schema()`/`segment_schema()` 返回类型改为自实现 `JsonSchemaFormat`，字段语义不变
- TTS 客户端简化为底层接口：
  - `TtsClient` trait 只提供 `chat_audio(messages, audio_format, voice) -> Result<ChatResponse>`，业务逻辑（guidance 注入、消息构造、base64 解码）由调用方负责
  - `AudioStage`/`Pipeline` 的 `tts_client` 字段改为 `&dyn TtsClient`（trait object）
  - `src/audio.rs` 内联 `synthesize_single` 辅助方法完成业务逻辑
  - 旧的 `src/tts/` 兼容模块已废弃（重导出 `crate::llm::TtsClient`，待清理删除）
- 依赖清理：`Cargo.toml` 删除 `llm`，新增 `async-stream`、`bytes`、`async-trait`（移至 `[dependencies]`），`reqwest` 启用 `stream` feature；`Cargo.lock` 已无 `llm` 子依赖
- 测试：
  - `tests/common/mod.rs` 改为基于 `ChatClient` trait 的 `MockProvider`，实现三方法（含流式 mock），并提供 `mock_response()` 构造 `ChatResponse`
  - `tests/pipeline_stages.rs` / `tests/full_pipeline.rs` 的 `test_config` 补全缺失字段
  - 验证：`cargo check --all-targets` 无 warning，`cargo test` 全部通过（单元 16 + pipeline_stages 8 + full_pipeline 6 + export_bindings 1 = 31 测试全绿）

### ⏳ 未完成（详见 PLAN.md）

#### Phase B：流水线解耦与进度事件
- [ ] 流水线阶段拆分为独立可调用阶段函数（按项目目录读写产物）
- [ ] `ProgressEvent` 接入 `AppState` 的 `broadcast::Sender`，SSE endpoint `GET /api/projects/:id/events`
- [ ] ~~`src/tts/client.rs` 双模式（design_voice / clone_voice）~~ — 方案已调整为：TTS 客户端走通用 `chat_audio`，业务上移到 `audio.rs`；voice_design/voice_clone 的具体落地待定

#### Phase C：API 端点
除 `projects` CRUD 之外，其余端点均未实现：
- [ ] 预处理端点（章节/摘要/段的 GET/PUT + 启动任务）
- [ ] 剧本端点（生成/单段重新生成/对话式修改/直接编辑）
- [ ] 音色设计端点（设计/试听/确认）
- [ ] 音频合成端点（按 action 序列处理 say/wait/play）
- [ ] SSE 进度端点

#### 待重构
- [ ] `src/script.rs` — 从 `ScriptLine` 重构为 `Action` 枚举（`Say`/`Wait`/`Play`），同步 `script_schema()` 改为 action-based
- [ ] `src/audio.rs` — 音频合成从逐行拼接改为 action 序列交叉拼接（TTS + 静音 + 音效）
- [ ] `src/sounds.rs`（新增）— 预置音效库管理（从 `assets/sounds/index.toml` 加载）
- [ ] `src/character.rs` — 添加 `#[derive(TS)]` 导出角色类型
- [ ] `src/checkpoint.rs` — 适配新阶段定义（preprocess/scripts/voices/audio）
- [ ] `prompts/script.md` / `prompts/story_teller.md` — 适配 action 序列输出

#### Phase D：React 前端
- [x] 项目脚手架存在（`frontend/` 目录 + `frontend/src/bindings/` 自动生成产物）
- [ ] 页面与路由（项目列表/仪表盘/预处理/剧本/音色/音频）
- [ ] 核心组件（`PhaseStepper`/`ChapterEditor`/`SegmentTimeline`/`ScriptEditor`/`RegeneratePanel`/`ChatPanel`/`CharacterCard`/`VoiceDesigner`/`AudioProgressBar`/`useEventStream`）

#### Phase E：配置与构建整合
- [x] 配置文件 `[server]` / `[workspace]` / `[timing]` / `[sounds]` 已落地
- [x] `[voice_design_model]` / `[voice_clone_model]` 字段已加（`Option<ModelConfig>`）
- [ ] CLI 子命令适配（`storyor server` / `storyor pipeline`）
- [ ] 构建脚本（开发并行 / 生产构建）

### 待清理
- [ ] `src/tts/` 目录（已废弃的死代码，重导出 `crate::llm::TtsClient`，未再被 lib.rs 引用，可直接删）

## v1 状态：CLI 流水线核心已完成 ✅

v1 的核心流水线（章节切分 → 章节摘要 → 剧情段切分 → 剧本生成 → TTS 合成 → 音频落盘 + 断点续跑）在 `src/pipeline/`、`src/novel/`、`src/audio.rs`、`src/checkpoint.rs` 中保留并已被新客户端接入。集成测试覆盖完整流水线与续跑。

## 模块结构（当前）

```
src/
  lib.rs              库入口
  main.rs              CLI 入口（server 模式）
  config.rs            全局 + 模型 + server/workspace/timing/sounds 配置
  error.rs             StoryorError（含 LLM/Server/Project 变体）
  project.rs           项目管理 CRUD
  ts_export.rs         ts-rs 全局注册表
  prompts.rs           提示词加载
  character.rs         角色库管理
  script.rs            剧本数据结构 + script_schema/segment_schema
  audio.rs             音频落盘 + manifest + TTS 业务侧（synthesize_single）
  checkpoint.rs        断点续跑
  novel/
    chapter.rs         章节切分
    mod.rs
  pipeline/
    mod.rs             流水线编排
    summary.rs         章节摘要
    segment.rs         剧情段切分
    script.rs          剧本生成
  llm/                 （新增）OpenAI 兼容客户端
    mod.rs             ChatClient / TtsClient trait
    types.rs           请求/响应类型
    client.rs          OpenAiClient 实现
    error.rs           OpenAiError 严格类型化
  server/
    mod.rs             axum 服务启动
    state.rs           AppState
    types.rs           API 类型
    events.rs          进度事件骨架
    routes/
      mod.rs           路由注册（projects CRUD + 健康）

frontend/
  src/
    bindings/          ts-rs 与 axfetchum 自动生成的 TS 类型 + API 客户端

prompts/                提示词模板
  story_teller.md
  summary.md
  segment.md
  script.md

tests/
  common/mod.rs         mock ChatClient + 测试数据
  export_bindings.rs   TS 绑定导出
  pipeline_stages.rs   三阶段集成测试
  full_pipeline.rs     完整流水线 + checkpoint 续跑
```

## 验证状态

- `cargo check --all-targets` 全工程通过，无 warning
- `cargo test` 31 个测试全部通过：
  - 单元测试 16 个（novel::chapter 5 + character 3 + checkpoint 2 + 其他 6）
  - 集成测试 `pipeline_stages` 8 个（summary/segment/script 三阶段 + 角色库累积 + 续跑）
  - 集成测试 `full_pipeline` 6 个（checkpoint 阶段级/段粒度续跑 + hash 变更重置 + force + 产物目录）
  - `export_bindings` 1 个（ts-rs 导出 + axfetchum 生成）

## 待办优先级建议

1. 清理 `src/tts/` 死代码
2. 完成 `script.rs` 的 Action 重构（连带 `audio.rs` 合成逻辑改造、`sounds.rs`、prompts 适配）
3. 接入 Phase B 的进度事件系统（`AppState` 广播 + SSE 端点）
4. 实现 Phase C 的剧本/音频端点
5. 启动 Phase D 前端开发
