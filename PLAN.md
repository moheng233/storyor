# Plan v2: 小说评书朗读生成器 (storyor)

## TL;DR

在 v1（CLI 单次流水线验证通过）基础上，升级为 **四阶段交互式工作流 + Web UI**。
- **Rust axum 后端** — REST API + SSE 进度推送
- **React SPA 前端** — Vite + TypeScript + [shadcn/ui](https://ui.shadcn.com/) 组件库，阶段式操作界面
- 四阶段独立可干预：预处理 → 剧本生成 → 音色设计 → 音频合成
- 音色分离设计：`voicedesign` 先行生成参考音频 → 用户确认 → `voiceclone` 精准复刻

## 架构总览

```
┌──────────────┐     HTTP/SSE      ┌──────────────┐
│  React SPA   │ ◄──────────────►  │  axum server │
│  (Vite+TS)   │    REST API       │  (Rust)      │
│  :5173       │                   │  :3001       │
└──────────────┘                   └──────┬───────┘
          ▲                               │
          │  ts-rs 生成 TS 类型            │
          │  axfetchum 生成 TS API 客户端   │
          │  (前后端类型复用，编译期校验)     │
          └───────────────────────────────┘
                                          │
                          ┌───────────────┼───────────────┐
                          │               │               │
                    ┌─────▼─────┐  ┌─────▼─────┐  ┌─────▼─────┐
                    │ 小模型     │  │ 大模型     │  │ TTS 模型  │
                    │ (摘要)     │  │ (剧本)     │  │ (音色+合成)│
                    └───────────┘  └───────────┘  └───────────┘
```

## 前后端类型安全策略（🔑 核心设计）

前后端类型一致性和 API 契约校验是整个系统可靠性的基石。Rust 端作为**单一事实来源（Single Source of Truth）**，所有共享类型和 API 接口定义在 Rust 侧，通过以下两个 crate 自动生成前端代码：

### ts-rs：Rust 类型 → TypeScript 类型

使用 `#[derive(TS)]` + `#[ts(export)]` 宏，在 `cargo test` 时自动将 Rust 数据结构导出为 TypeScript 类型定义文件。

```rust
// src/script.rs
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]  // 自动导出到 bindings/ 目录
pub struct Script {
    pub segment_index: usize,
    pub characters: CharacterLibrary,
    pub paragraphs: Vec<Paragraph>,
    pub handoff: String,
}
```

导出产物示例：
```typescript
// frontend/src/bindings/Script.ts
export interface Script {
    segment_index: number;
    characters: CharacterLibrary;
    paragraphs: Array<Paragraph>;
    handoff: string;
}
```

### axfetchum：Axum 路由 → TypeScript API 客户端

使用 `api_routes!` 宏声明式定义路由元数据（路径、HTTP 方法、请求体/响应体类型），自动生成带完整类型的 TypeScript fetch 封装函数。

```rust
// src/server/routes/scripts.rs
use axfetchum::{api_routes, RouteCollection};

fn script_routes() -> RouteCollection {
    api_routes! {
        @group scripts

        getScripts:   GET  "/projects/{id}/scripts"
            -> Vec<ScriptMeta>;
        getScript:    GET  "/projects/{id}/scripts/{seg_idx}"
            -> Script;
        updateScript: PUT  "/projects/{id}/scripts/{seg_idx}"
            body: Script -> Script;
    }
}
```

生成的 TypeScript 客户端：
```typescript
// frontend/src/bindings/api.ts
import type { Script, ScriptMeta } from "./Script";

export function getScripts(id: string): Promise<Array<ScriptMeta>> { /* ... */ }
export function getScript(id: string, segIdx: number): Promise<Script> { /* ... */ }
export function updateScript(id: string, segIdx: number, body: Script): Promise<Script> { /* ... */ }
```

### 工作流

1. **Rust 端定义**：用 `#[derive(TS)]` 标注所有前后端共享的数据结构，用 `api_routes!` 声明所有 API 端点
2. **自动生成**：`cargo test`（或独立 `export_bindings` 步骤）触发 ts-rs 导出 TS 类型定义 + axfetchum 生成 TS API 客户端
3. **前端消费**：前端直接 `import { Script } from '@/bindings/Script'` 和 `import { getScript } from '@/bindings/api'`，享受完整类型推导
4. **CI 校验**：axfetchum 的 `check()` 函数可在 CI 中检测生成代码是否过期，防止前后端类型漂移

## 四阶段交互流程

| 阶段 | 做什么 | 用户可修改 |
|------|--------|-----------|
| **1. 预处理** | 章节切分 → 逐章摘要 → 剧情段划分 | 章节边界、摘要文本、段范围 |
| **2. 剧本生成** | 逐段生成剧本（角色库+台词+衔接话） | 三种修改方式：附加提示词重新生成 / 对话式修改 / 直接编辑 |
| **3. 音色设计** | `voicedesign` 按 guidance 生成参考音频 | guidance 文本、重新生成、确认音色 |
| **4. 音频合成** | `voiceclone` + 已确认音色，**按剧情段**逐段合成最终音频 | 单段重新合成、单句重新合成、进度监控 |

### 阶段 2 详解：剧本段的三种修改模式

每个剧情段支持独立操作，用户可对任一段反复打磨：

| 模式 | 说明 | 适用场景 |
|------|------|----------|
| **① 附加提示词重新生成** | 在原始 prompt 基础上追加用户自定义指令（如"让岑清霜更傲娇一点"），大模型根据原文 + 已有角色库 + 用户提示词重新生成该段完整剧本 | 对整体风格/角色性格不满意，需要大改 |
| **② 对话式修改** | 以当前剧本为上下文，打开一个对话面板。用户用自然语言提出修改意见（如"把第三段对话改得更温柔些"），大模型理解上下文后返回修改后的剧本 | 局部调整、不确定具体怎么改，信任模型判断 |
| **③ 直接编辑** | 纯前端文本编辑器，直接修改 JSON 中的台词、角色档案、段落分组、handoff。支持行内编辑和原始 JSON 编辑两种视图 | 精细微调、修正明显错误、手动补全 |

三种模式共享同一个剧本编辑器界面，用户可随时切换。每次修改后自动落盘，角色库随之更新。

## 用户决策

- **UI 方案**：Rust axum 后端 + React SPA 前端（前后端分离）
- **音色 API**：`voicedesign` 返回参考音频样本 → `voiceclone` 通过该音频做 few-shot 克隆
- **数据持久化**：延续文件系统方案（JSON + 目录），每个项目独立目录
- **部署方式**：仅本地运行，无需鉴权/多用户
- 其余继承 v1 决策（章节正则、角色库顺序传递、断点续跑、提示词外置）

## 项目产物目录结构（每个项目独立）

在 `workspace_dir/` 下按项目名分目录。每个项目目录延续 v1 `output/` 格式，新增 `voices/` 子目录。

```
<workspace_dir>/
└── <project_name>/
    ├── project.json                # 项目元信息（id/name/novel_path/phase/config）
    ├── chapters.json               # 章节切分结果
    ├── summaries.json              # 章节摘要
    ├── segments.json               # 剧情段切分
    ├── characters/
    │   └── final.json              # 最终累积角色库
    ├── voices/
    │   ├── 角色A/
    │   │   ├── ref_audio.mp3       # voicedesign 生成的参考音频
    │   │   └── voice.json          # { guidance, confirmed: bool, created_at }
    │   └── 角色B/
    │       └── ...
    ├── scripts/
    │   ├── segment_0001.json
    │   ├── segment_0001.handoff.txt
    │   └── ...
    ├── audio/
    │   ├── segment_0001/
    │   │   ├── p0000_l0000.mp3
    │   │   ├── segment.wav
    │   │   └── ...
    │   └── ...
    ├── manifest.json
    └── checkpoint.json
```

### checkpoint.json（v2 扩展）

```json
{
  "novel_hash": "<sha256>",
  "config_hash": "<sha256>",
  "stages": {
    "preprocess": "done",
    "scripts":    { "completed": [0, 1], "total": 5 },
    "voices":     { "completed": ["岑清霜", "旁白"], "total": 8 },
    "audio":      { "completed": [0],    "total": 5 }
  }
}
```

## 实施步骤

### Phase A：后端基础设施

**A1. 添加依赖** (`Cargo.toml`)
- 新增：`axum`, `tower`, `tower-http` (cors/serve static), `uuid`, `sha2` (已有)
- **新增前后端类型桥接**：`ts-rs` (v12, `#[derive(TS)]` 导出 TS 类型), `axfetchum` (v0.1, `api_routes!` 生成 TS API 客户端)
- 可能新增 `[[bin]]` 或保持单 binary 多子命令

**A2. 创建 server 模块**
- `src/server/mod.rs` — `run_server(config)` 启动 axum，监听 `127.0.0.1:3001`
- `src/server/state.rs` — `AppState`：`ProjectManager` + `Config` + 进度 `broadcast::Sender`
- `src/server/routes/mod.rs` — 路由注册，同时通过 `axfetchum::api_routes!` 声明每个路由的元数据（路径/方法/请求体/响应体类型），自动生成 TS API 客户端
- `src/server/types.rs` — 集中定义所有 API 请求/响应类型，全部标注 `#[derive(TS, Serialize, Deserialize)]`
- 生产模式：axum serve `frontend/dist/` 静态文件 + SPA fallback

**A3. 项目管理模块**
- `src/project.rs` — `ProjectManager`：
  - `workspace_dir` 下按项目名分目录
  - `ProjectMeta { id, name, novel_path, created_at, current_phase, config }` 存 `project.json`
  - `list_projects()`, `create_project()`, `get_project()`, `delete_project()`

### Phase B：流水线解耦与进度事件

**B1. 流水线阶段独立化**
- 将 `src/pipeline/mod.rs` 中 `Pipeline::run()` 的串联逻辑拆分为独立可调用阶段函数
- 每个阶段函数接受 `project_id`，自行读取/写入项目目录下的产物文件
- 阶段间通过文件系统传递数据

**B2. 进度事件系统**
- `src/server/events.rs` — `ProgressEvent` 枚举（`StageStarted/StageProgress/StageCompleted/StageError`）
- `AppState` 持有 `broadcast::Sender<ProgressEvent>`，各阶段函数通过它推送进度
- SSE endpoint `GET /api/projects/:id/events`

**B3. TTS 客户端双模式改造**
- `src/tts/client.rs` 新增两种方法：
  - `design_voice(guidance: &str) -> Result<Vec<u8>>` — 调 `voicedesign` 模型，返回参考音频
  - `clone_voice(reference_audio: &[u8], text: &str) -> Result<Vec<u8>>` — 调 `voiceclone` 模型
- 配置文件新增 `[voice_design_model]` 和 `[voice_clone_model]` 两个 `ModelConfig`

### Phase C：API 端点

**C1. 项目管理** (`src/server/routes/projects.rs`)
- `GET /api/projects` — 项目列表
- `POST /api/projects` — 创建项目（multipart: novel file + config）
- `GET /api/projects/:id` — 项目详情
- `DELETE /api/projects/:id` — 删除项目

**C2. 预处理** (`src/server/routes/preprocess.rs`)
- `POST /api/projects/:id/preprocess` — 启动预处理（异步，SSE 报告进度）
- `GET /api/projects/:id/chapters` — 获取章节列表
- `PUT /api/projects/:id/chapters/:idx` — 修改章节
- `GET /api/projects/:id/summaries` — 获取摘要列表
- `PUT /api/projects/:id/summaries/:idx` — 修改摘要
- `GET /api/projects/:id/segments` — 获取剧情段
- `PUT /api/projects/:id/segments/:idx` — 修改剧情段

**C3. 剧本** (`src/server/routes/scripts.rs`)
- `POST /api/projects/:id/scripts/generate` — 生成全部剧本（异步）
- `POST /api/projects/:id/scripts/:segIdx/regenerate` — **单段重新生成**（body: `{ extra_prompt: string }`；将该段的额外提示词追加到原始 prompt，大模型生成新剧本覆盖旧产物，角色库随之更新）
- `POST /api/projects/:id/scripts/:segIdx/chat` — **对话式修改**（body: `{ user_message: string }`；以当前剧本为上下文，大模型根据用户消息返回修改后的该段剧本）
- `GET /api/projects/:id/scripts` — 获取剧本元信息列表
- `GET /api/projects/:id/scripts/:segIdx` — 获取单段完整剧本
- `PUT /api/projects/:id/scripts/:segIdx` — **直接编辑保存**（前端修改后提交完整 Script JSON，服务端校验并落盘）
- `GET /api/projects/:id/characters` — 获取角色库
- `PUT /api/projects/:id/characters/:name` — 修改角色

**C4. 音色设计** (`src/server/routes/voices.rs`)
- `GET /api/projects/:id/voices` — 获取所有角色音色状态
- `POST /api/projects/:id/voices/:charName/design` — 生成参考音频（异步）
- `GET /api/projects/:id/voices/:charName/sample` — 获取参考音频文件
- `PUT /api/projects/:id/voices/:charName/guidance` — 修改 guidance 并重新设计
- `POST /api/projects/:id/voices/:charName/confirm` — 确认音色

**C5. 音频合成** (`src/server/routes/audio.rs`)
- `POST /api/projects/:id/audio/synthesize` — 合成全部未完成段落的音频（异步）
- `POST /api/projects/:id/audio/segments/:segIdx/synthesize` — **单段合成**：仅合成指定剧情段的全部台词（异步，支持 `extra_prompt` 注入合成参数）
- `GET /api/projects/:id/audio/status` — 合成进度（按段粒度展示：哪些段已完成、当前正在合成哪段哪句）
- `GET /api/projects/:id/audio/segments/:segIdx/paragraphs/:paraIdx` — 获取段落音频（单次 TTS 调用产物）
- `GET /api/projects/:id/audio/segments/:segIdx/segment.wav` — 获取合并音频
- `GET /api/projects/:id/manifest` — 获取音频清单

**C6. SSE 进度端点**
- `GET /api/projects/:id/events` — Server-Sent Events 推送所有阶段进度

### Phase D：React 前端

**D1. 项目初始化**
- `frontend/` 目录，Vite + React + TypeScript
- **UI 组件库**：[shadcn/ui](https://ui.shadcn.com/)（基于 Radix UI + Tailwind CSS），提供 Button、Card、Dialog、Tabs、Textarea、Slider、Badge、Toast、Sheet 等开箱即用的无障碍组件
- 样式：Tailwind CSS（shadcn/ui 默认依赖）
- 依赖：`react-router-dom`, `@tanstack/react-query`, `lucide-react`（shadcn/ui 默认图标库）
- 开发代理：`/api` → `localhost:3001`
- **类型导入**：前端直接从 `frontend/src/bindings/` 导入 ts-rs 生成的类型定义 + axfetchum 生成的 API 客户端，零手写 API 类型

**D2. 页面与路由**
| 路由 | 页面 | 核心功能 |
|------|------|----------|
| `/` | 项目列表 | 项目卡片、新建项目对话框（上传小说+配置） |
| `/project/:id` | 项目仪表盘 | 四阶段 Stepper、当前阶段状态、快捷操作按钮 |
| `/project/:id/preprocess` | 预处理 | 章节列表编辑、摘要编辑、段时间轴（可拖拽调整范围） |
| `/project/:id/scripts` | 剧本 | 段列表 + 剧本编辑器（三种模式切换）+ 角色库面板、Handoff 编辑 |
| `/project/:id/voices` | 音色设计 | 角色卡片网格、guidance 编辑器、音频播放器、确认按钮 |
| `/project/:id/audio` | 音频合成 | 段列表（每段可折叠展开段落进度），逐句播放测试，**支持按段独立合成/重新合成** |

**D3. 核心组件**（基于 shadcn/ui）
- `PhaseStepper` — 四阶段步骤指示器（基于 `Tabs` / `Stepper` 自定义）
- `ChapterEditor` — 章节列表 + 行内编辑（`Table` + `Input`）
- `SegmentTimeline` — 剧情段时间轴（可拖拽调整范围，`Slider` + `Card`）
- `ScriptEditor` — 剧本编辑器（段落折叠 `Collapsible`、台词行内编辑 `Textarea`、原始 JSON 编辑双视图）
- `RegeneratePanel` — 单段重新生成面板（额外提示词 `Textarea` + `Button` + 进度 `Progress`）
- `ChatPanel` — 对话式修改面板（聊天气泡界面，`ScrollArea` + `Input`，用户自然语言输入）
- `CharacterCard` — 角色卡片（`Card` + `Badge` 状态标签）
- `VoiceDesigner` — guidance 文本框 + 音频播放器 + `Button` 操作组
- `AudioProgressBar` — 合成进度（`Progress` + `Table` 段/段落折叠列表，逐句播放按钮）
- `useEventStream` — SSE 事件订阅 hook

### Phase E：配置与构建整合

**E1. 配置扩展** (`storyor.toml`)
```toml
# 新增
[voice_design_model]
backend = "OpenAI"
api_key = "..."
base_url = "..."
model = "mimo-v2.5-tts-voicedesign"

[voice_clone_model]
backend = "OpenAI"
api_key = "..."
base_url = "..."
model = "mimo-v2.5-tts-voiceclone"

[server]
host = "127.0.0.1"
port = 3001

[workspace]
dir = "./workspace"
```

**E2. CLI 适配**
- `storyor server` — 启动 Web UI 服务器
- `storyor pipeline --input <file>` — 保留原 CLI 批处理模式

**E3. 构建脚本**
- 开发：`cargo run -- server` + `cd frontend && npm run dev` 并行
- 生产：`cd frontend && npm run build` → `cargo build --release`（axum serve 静态文件）

## 模块结构（v2 总览）

```
src/
  main.rs              CLI 入口（server / pipeline 子命令）
  config.rs            配置扩展（voice_design/clone/server/workspace）
  project.rs           项目管理 CRUD
  error.rs             错误类型（新增 Server/Project 变体）
  novel/chapter.rs     章节切分（复用 v1）
  pipeline/
    mod.rs             流水线编排 → 拆分为独立阶段函数
    summary.rs         章节摘要（复用核心逻辑）
    segment.rs         剧情段切分（复用核心逻辑）
    script.rs          剧本生成（支持单段独立生成）
  server/
    mod.rs             axum 服务启动
    state.rs           AppState（项目管理器 + 进度广播）
    types.rs           API 请求/响应类型（#[derive(TS)] 自动导出 TS 类型）
    events.rs          进度事件定义 + SSE 辅助
    routes/
      mod.rs           路由注册 + axfetchum::api_routes! 声明
      projects.rs      项目管理端点
      preprocess.rs    预处理端点
      scripts.rs       剧本端点
      voices.rs        音色设计端点
      audio.rs         音频合成端点
  character.rs         角色库管理（复用 v1，新增 #[derive(TS)]）
  script.rs            数据结构（新增 #[derive(TS)] 导出 TS 类型）
  prompts.rs           提示词加载（复用 v1）
  tts/
    mod.rs
    client.rs          VoiceDesign + VoiceClone 双模式
  audio.rs             音频落盘合并（复用核心逻辑）
  checkpoint.rs        断点续跑（适配新阶段定义）

prompts/
  story_teller.md      说书人风格系统提示词
  summary.md           章节摘要提示词
  segment.md           剧情段切分提示词
  script.md            剧本生成提示词

frontend/
  src/
    bindings/          ts-rs 生成的 TS 类型 + axfetchum 生成的 API 客户端
    pages/             项目列表 / 仪表盘 / 预处理 / 剧本 / 音色 / 音频
    components/        共享组件
    hooks/             useEventStream / useApi
    api/               API client 封装（基于 axfetchum 生成代码）
```

## 关键文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `Cargo.toml` | 修改 | 新增 axum/tower-http/uuid/ts-rs/axfetchum |
| `src/main.rs` | 修改 | 新增 `server` 子命令 |
| `src/config.rs` | 修改 | 新增 voice_design/clone/server/workspace 配置 |
| `src/error.rs` | 修改 | 新增 Server/Project 错误变体 |
| `src/tts/client.rs` | 修改 | VoiceDesign + VoiceClone 双模式 |
| `src/pipeline/mod.rs` | 修改 | 拆分为独立阶段函数 |
| `src/pipeline/script.rs` | 修改 | 支持单段独立生成 + 单段重新生成（extra_prompt）+ 对话式修改（chat context） |
| `src/checkpoint.rs` | 修改 | 适配新阶段定义（preprocess/scripts/voices/audio） |
| `src/script.rs` | 修改 | 新增 VoiceRef 字段，添加 `#[derive(TS)]` |
| `src/character.rs` | 修改 | 添加 `#[derive(TS)]` 导出角色类型 |
| `src/server/mod.rs` | **新增** | axum 服务启动 |
| `src/server/state.rs` | **新增** | 共享状态 |
| `src/server/types.rs` | **新增** | API 请求/响应类型（`#[derive(TS)]`） |
| `src/server/events.rs` | **新增** | 进度事件 |
| `src/server/routes/*.rs` | **新增** | 5 组 API 端点 + `api_routes!` 声明 |
| `src/project.rs` | **新增** | 项目管理 CRUD |
| `frontend/` | **新增** | React 项目（shadcn/ui + Tailwind CSS + TypeScript）+ bindings/ 自动生成的 TS 代码 |

## Decisions

- **音频合成分段独立**：与剧本段一一对应，每段可独立触发合成，不依赖其他段。checkpoint 按段粒度记录音频完成状态，支持单段重新合成（覆盖旧产物）
- **前后端类型安全**：Rust 为单一事实来源，`ts-rs`（`#[derive(TS)]`）自动导出 TS 类型定义，`axfetchum`（`api_routes!`）自动生成带类型的 TS API 客户端。前端代码零手写 API 类型，编译期保证前后端契约一致
- **音色设计工作流**：`voicedesign` 返回音频样本 → 用户试听 → 修改 guidance 重新生成 → 确认后锁定 → `voiceclone` 以该音频为参考做 few-shot 克隆
- **TTS 双模式**：`TtsClient` 设计两个独立方法（`design_voice` / `clone_voice`），具体 HTTP 请求格式通过 `ModelConfig` 中的 `extra_body` 扩展
- **前后端分离**：开发时 Vite dev server (5173) 代理 API → axum (3001)；生产构建 axum serve 静态文件
- **进度推送**：SSE（Server-Sent Events）单向推送，`tokio::sync::broadcast` 实现
- **数据持久化**：文件系统 JSON + 目录，每个项目独立 workspace 子目录，无需数据库
- 其余继承 v1 决策

## Verification

1. `cargo build` 编译通过（server + pipeline 模式）
2. `cargo test` 现有 24 测试保持通过 + ts-rs 导出 bindings 自动执行
3. **类型一致性**：`cargo test` 后检查 `frontend/src/bindings/` 目录生成正确，`tsc --noEmit` 无类型错误
4. **UI 组件**：shadcn/ui 组件正常渲染，Tailwind CSS 样式生效，深色/浅色主题切换正常
4. 手动：`storyor server` → 浏览器 `localhost:3001` → 项目列表渲染
5. 手动：创建项目 → 上传小说 → 预处理 → 编辑章节/摘要/段 → 确认
6. 手动：剧本生成 → 编辑台词/角色 → 角色库正确累积更新
7. 手动：音色设计 → guidance 生成参考音频 → 播放 → 修改 → 确认
8. 手动：音频合成 → 单段合成 → voiceclone 逐句合成 → 播放 → 段合并 WAV → 另一段单独重新合成验证独立操作
9. 手动：中途关闭浏览器 → 重启 server → checkpoint 续跑（段粒度音频续跑）

## Further Considerations

1. **voicedesign / voiceclone API 细节待确认**：具体 HTTP 请求/响应格式需要在首次调试时确定，当前通过 `extra_body` 灵活适配
2. **前端富文本编辑**：台词 content 含内联标签如 `（怅然）（深呼吸）`，剧本编辑器初期用纯文本框，后续可考虑可视化标签插入
3. **异步任务生命周期**：剧本生成和音频合成耗时较长，需用 `tokio::spawn` 异步执行，通过 SSE 推送进度；任务不持久化到数据库，仅通过文件系统 checkpoint 判断完成情况
4. **错误恢复**：每个阶段/段/句的失败应可重试，不影响已完成部分
5. **ts-rs 与 axfetchum 集成**：
   - 构建流程中增加 `cargo test`（触发 ts-rs 导出）和 axfetchum 生成的检查步骤
   - CI 中用 `axfetchum::check()` 防止手动修改生成文件导致前后端类型漂移
   - 前端 `package.json` 中添加 `generate:bindings` 脚本，一键运行 Rust 端的类型导出
6. **对话式修改的上下文管理**：`POST /scripts/:segIdx/chat` 端点每次收到用户消息时，需构造完整的对话上下文（系统提示词 + 当前剧本完整 JSON + 之前的修改对话历史），大模型才能在充分理解现状的基础上给出精准修改。对话历史不需要持久化（每次会话独立），但当前剧本作为每次请求的必要上下文始终注入

