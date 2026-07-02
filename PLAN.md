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
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Action {
    Say {
        speaker: String,
        content: String,
        description: String,
    },
    Wait {
        duration: String,  // "short" | "medium" | "long"
    },
    Play {
        sound: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Paragraph {
    pub index: usize,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
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

## 🔑 剧本格式重构（v2 重大变更）

### 设计动机

当前 v1 剧本格式中，`ScriptLine.content` 需要内联拟声词（如 `（敲桌）`、`（巨大爆炸声）`），但 TTS 模型无法生成拟声词/音效 —— 它只能合成人声。同时现有格式无法精确控制停顿，导致音频节奏无法调整。

因此 v2 将剧本从 **纯台词列表** 重构为 **动作序列（Action Sequence）**，在时间线上精确编排：说话、停顿、音效。

### 新格式：Action 序列

```
Script (per segment)
├── characters: CharacterLibrary      // 角色库（不变）
├── paragraphs: [                     // 段落分组保留（情绪/场景单位）
│   {
│     "index": 0,
│     "actions": [                    // ← 原 lines → 改为 actions
│       { "type": "say",   ... },     // 说话
│       { "type": "wait",  ... },     // 停顿
│       { "type": "play",  ... },     // 音效
│       { "type": "say",   ... },
│     ]
│   }
│ ]
└── handoff: string                   // 衔接话（不变）
```

### 三种 Action 类型

#### 1. `say` — 角色台词 / 旁白

TTS 人声合成的唯一输入。`content` 为**台词文本，允许括号包裹的人声演绎提示，但禁止人声无法表现的拟声词/音效**。

```json
{
  "type": "say",
  "speaker": "岑清霜",
  "content": "（紧张，深呼吸）呼……冷静，冷静。",
  "description": "刚刚经历一场恶战，呼吸急促，努力平复心情，声音微微发颤"
}
```

| 字段 | 说明 |
|------|------|
| `speaker` | 说话者（角色名或 `"旁白"`） |
| `content` | 台词文本，**允许**括号包裹的人声演绎提示（TTS `（风格）` 和 `[音频标签]`），包括但不限于：情绪（`（紧张）`、`（怅然）`）、语调（`（低沉）`、`（慵懒）`）、呼吸（`（深呼吸）`、`（长叹一口气）`）、哭笑（`（冷笑）`、`（哽咽）`）、语速（`（语速加快）`）、语音特征（`（颤抖）`、`（气声）`）、方言（`（东北话）`）等。**禁止**人声无法表现的拟声词/音效（如 `（敲桌）`、`（爆炸声）`、`（风声）`、`（脚步）`）。直接作为 TTS 的 assistant 消息 |
| `description` | 导演模式演绎指导（100-300字），合并入 TTS 的 user 消息 |

**与 v1 的关键区别**：`content` 从 `"（紧张，深呼吸）（敲桌）呼……冷静，冷静。"` 变为 `"（紧张，深呼吸）呼……冷静，冷静。"` —— 人声演绎提示保留，但非人声拟声词/音效从 content 中分离，转为独立的 `play` 动作。

#### 2. `wait` — 停顿控制

在时间线中插入静音段，控制节奏。

```json
{ "type": "wait", "duration": "short" }
```

`duration` 使用**语义标签**，由配置文件映射为实际秒数：

```toml
[timing]
short_pause_secs = 0.5    # 短停顿（逗号、换气）
medium_pause_secs = 1.5   # 中停顿（句间、场景微转）
long_pause_secs = 3.0     # 长停顿（场景转换、悬念）
```

| 标签 | 适用场景 |
|------|----------|
| `"short"` | 逗号/换气/话锋微转 |
| `"medium"` | 句间停顿/情绪切换 |
| `"long"` | 场景转换/悬念留白/章节分隔 |

#### 3. `play` — 音效触发

在时间线中插入预置音效文件。

```json
{ "type": "play", "sound": "knock_door" }
```

`sound` 对应预置音效库中的文件名（不含扩展名）。若找不到对应文件，生成等长静音占位并警告。

### 完整示例

```json
{
  "segment_index": 0,
  "paragraphs": [
    {
      "index": 0,
      "actions": [
        {
          "type": "say",
          "speaker": "旁白",
          "content": "夜色如墨，一道黑影掠过屋檐。",
          "description": "评书口吻，压低声音制造悬念，语速稍慢"
        },
        { "type": "wait", "duration": "medium" },
        {
          "type": "play",
          "sound": "wind_howl"
        },
        { "type": "wait", "duration": "short" },
        {
          "type": "say",
          "speaker": "岑清霜",
          "content": "谁在那里？",
          "description": "警觉、戒备，声音清冷而锐利"
        },
        { "type": "wait", "duration": "short" },
        {
          "type": "play",
          "sound": "footsteps_stone"
        }
      ]
    }
  ],
  "handoff": "岑清霜发现有人跟踪，准备迎战……"
}
```

### LLM 提示词变更

- **`prompts/script.md`**：指示大模型输出 action 序列（`say`/`wait`/`play`），在合适的时机插入停顿和音效。`say.content` 允许 TTS 的 `（风格）` 和 `[音频标签]`（情绪、语调、呼吸、哭笑、语速、语音特征、方言等），但人声无法表现的拟声词/音效必须通过 `play` 动作单独表达。
- **`prompts/story_teller.md`**：新增系统指令：理解停顿标签和音效触发规则。
- **JSON Schema**：`script_schema()` 重构为 action-based schema，使用 `oneOf` / `anyOf` 表达三种 action 类型。

### 音频合成流程变化

```
当前 v1（逐行合成 → 拼接）：
  for line in paragraph.lines:
      mp3 = tts.synthesize(line.content)
  → concat all mp3s → segment.wav

v2（动作序列合成 → 交叉拼接）：
  for action in paragraph.actions:
      match action:
          Say  → mp3 = tts.synthesize(action.content, action.description)
          Wait → silence = generate_silence(action.duration)
          Play → sfx = load_sound_effect(action.sound)
  → concat交替拼接(say_mp3 + silence + sfx + ...) → segment.wav
```

- **TTS 调用不变**：`synthesize_line` 仍然接收 `content` + `description`，`content` 保留人声演绎提示（`（紧张）`等），仅移除人声无法表现的音效标注
- **Wait 处理**：生成指定时长的静音 PCM 数据（采样率/声道从相邻 TTS 产物探测）
- **Play 处理**：从预置音效库加载文件，解码为 PCM 后插入

### 预置音效库

```
assets/sounds/
├── index.toml              # 音效名 → 文件名 + 元信息
├── knock_door.mp3
├── wind_howl.mp3
├── footsteps_stone.mp3
├── explosion.mp3
├── thunder.mp3
├── ...
└── silence_1s.mp3          # fallback 静音
```

`index.toml` 格式：

```toml
[sounds.knock_door]
file = "knock_door.mp3"
description = "敲门声（木门）"
category = "日常"

[sounds.wind_howl]
file = "wind_howl.mp3"
description = "呼啸风声"
category = "环境"
```

### 前端编辑器适配

剧本编辑器需要适配新的 action 列表结构：

- **ActionTimeline** 组件：可视化时间线，展示 say/wait/play 动作序列
- say 行内编辑：speaker 下拉 + content 文本框（允许括号人声提示，禁止音效标注）+ description 可折叠
- wait 行内编辑：duration 下拉（short/medium/long）
- play 行内编辑：sound 下拉（从音效库 index 读取可用列表）+ 试听按钮
- 支持拖拽重排 action 顺序
- 原始 JSON 编辑视图同步更新

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
    │   │   ├── p0000_a0000.mp3       # say action 的 TTS 产物
    │   │   ├── p0000_a0001.sil.mp3   # wait 生成的静音段
    │   │   ├── segment.wav            # action 序列交叉拼接后的完整 WAV
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
- `POST /api/projects/:id/audio/segments/:segIdx/synthesize` — **单段合成**：按 action 序列依次处理（say→TTS合成 / wait→插入静音 / play→加载音效文件），异步
- `GET /api/projects/:id/audio/status` — 合成进度（按段粒度展示：哪些段已完成、当前正在合成哪段的哪个 action）
- `GET /api/projects/:id/audio/segments/:segIdx/actions/:actionIdx` — 获取单个 action 的音频产物（say 的 TTS 产物 / play 的音效文件）
- `GET /api/projects/:id/audio/segments/:segIdx/segment.wav` — 获取合并音频（action 序列交叉拼接后的完整 WAV）
- `GET /api/projects/:id/manifest` — 获取音频清单
- `GET /api/projects/:id/sounds` — 获取可用音效列表（从 `assets/sounds/index.toml` 读取）

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

[timing]
short_pause_secs = 0.5     # 短停顿
medium_pause_secs = 1.5    # 中停顿
long_pause_secs = 3.0      # 长停顿

[sounds]
dir = "./assets/sounds"    # 预置音效库目录
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
| `src/config.rs` | 修改 | 新增 voice_design/clone/server/workspace/timing/sounds 配置 |
| `src/error.rs` | 修改 | 新增 Server/Project 错误变体 |
| `src/tts/client.rs` | 修改 | VoiceDesign + VoiceClone 双模式 |
| `src/pipeline/mod.rs` | 修改 | 拆分为独立阶段函数；音频合成改为 action 序列驱动 |
| `src/pipeline/script.rs` | 修改 | 支持单段独立生成 + 单段重新生成（extra_prompt）+ 对话式修改（chat context） |
| `src/checkpoint.rs` | 修改 | 适配新阶段定义（preprocess/scripts/voices/audio） |
| `src/script.rs` | **重构** | TextLine → Action 枚举（say/wait/play），Schema 重构，新增 `#[derive(TS)]` |
| `src/character.rs` | 修改 | 添加 `#[derive(TS)]` 导出角色类型 |
| `src/audio.rs` | **重构** | 逐行拼接 → action 序列交叉拼接（TTS + 静音 + 音效），新增 wait 生成和 play 解码 |
| `src/sounds.rs` | **新增** | 音效库管理（从 `assets/sounds/index.toml` 加载，按名称查找文件） |
| `src/server/mod.rs` | **新增** | axum 服务启动 |
| `src/server/state.rs` | **新增** | 共享状态 |
| `src/server/types.rs` | **新增** | API 请求/响应类型（`#[derive(TS)]`） |
| `src/server/events.rs` | **新增** | 进度事件 |
| `src/server/routes/*.rs` | **新增** | 5 组 API 端点 + `api_routes!` 声明 |
| `src/project.rs` | **新增** | 项目管理 CRUD |
| `prompts/script.md` | **修改** | 输出格式从台词列表改为 action 序列 |
| `prompts/story_teller.md` | **修改** | 新增停顿标签和音效触发规则的系统指令 |
| `assets/sounds/index.toml` | **新增** | 预置音效库索引 |
| `frontend/` | **新增** | React 项目（shadcn/ui + Tailwind CSS + TypeScript）+ bindings/ 自动生成的 TS 代码 |

## Decisions

- **剧本格式重构**：台词从 `ScriptLine` 改为 `Action` 枚举（`Say`/`Wait`/`Play`）。`Say.content` 保留人声演绎提示（`（紧张）`等），非人声拟声词/音效通过 `Play` 独立表达，停顿通过 `Wait` 精确控制
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
2. **前端 ActionTimeline 编辑器**：剧本编辑器需从台词列表视图改为时间线视图，展示 say/wait/play 动作序列；say.content 保留人声提示（`（紧张）`等），wait 和 play 用不同颜色标注
3. **音效库覆盖度**：初期预置基础音效（风雨、敲门、爆炸、脚步等），后续根据实际剧本需求逐步扩充
4. **异步任务生命周期**：剧本生成和音频合成耗时较长，需用 `tokio::spawn` 异步执行，通过 SSE 推送进度；任务不持久化到数据库，仅通过文件系统 checkpoint 判断完成情况
5. **错误恢复**：每个阶段/段/句的失败应可重试，不影响已完成部分
6. **ts-rs 与 axfetchum 集成**：
   - 构建流程中增加 `cargo test`（触发 ts-rs 导出）和 axfetchum 生成的检查步骤
   - CI 中用 `axfetchum::check()` 防止手动修改生成文件导致前后端类型漂移
   - 前端 `package.json` 中添加 `generate:bindings` 脚本，一键运行 Rust 端的类型导出
7. **对话式修改的上下文管理**：`POST /scripts/:segIdx/chat` 端点每次收到用户消息时，需构造完整的对话上下文（系统提示词 + 当前剧本完整 JSON + 之前的修改对话历史），大模型才能在充分理解现状的基础上给出精准修改。对话历史不需要持久化（每次会话独立），但当前剧本作为每次请求的必要上下文始终注入

---

## 子计划：用 reqwest 替换 llm crate 实现 OpenAI chat_completions 客户端

当前代码通过 `llm` crate（v1.3.8）调用大模型。该 crate 的 `ChatProvider` trait 被 `pipeline` 三阶段（summary/segment/script）使用，测试中也通过 mock `ChatProvider` 注入响应。目标是用项目已有的 `reqwest` 直接实现 OpenAI `/v1/chat/completions` 请求/响应，移除 `llm` 依赖，并新增可返回 token 流的 `chat_stream` 方法，供后续 Web UI 聊天式改稿使用。

**推荐方案**：新增独立 `src/llm/` 模块，自包含 OpenAI 兼容客户端。流水线三阶段改为依赖自定义的 `ChatClient` trait/struct；config 移除 `parse_backend`，保留 `backend` 字段作日志/展示。结构化输出继续走 `response_format: json_schema` 严格模式，保持与现有 schema 函数（`script_schema`/`segment_schema`）兼容。

### Phase 1：梳理与类型设计

1. 在 `src/llm/` 下创建：
   - `mod.rs` — 模块入口，暴露 `ChatClient`、`ChatMessage`、`ChatResponse`、`ChatStream` 等。
   - `client.rs` — `OpenAiClient` 实现：`new(config: &ModelConfig)` 构造 reqwest Client；`chat(...)`/`chat_stream(...)` 发送请求。
   - `types.rs` — 请求/响应类型：手动实现 OpenAI schema（roles、messages、response_format json_schema、choices、usage、finish_reason、OpenAIError 等）。
2. 在 `src/script.rs` 中：
   - 移除 `use llm::chat::StructuredOutputFormat`。
   - 保留 `script_schema()`/`segment_schema()` 的返回类型，改为自实现的 `JsonSchemaFormat`（或等价结构），保持字段语义：name、description、schema、strict。
3. 在 `src/config.rs` 中：
   - 移除 `llm::builder::LLMBackend` 导入。
   - 移除/弃用 `ModelConfig::parse_backend()`，仅保留 `backend: String` 字段用于展示。
4. 保留 `src/error.rs` 中的 `StoryorError::Llm(String)` 变体，新增 `From` 转换。

### Phase 2：流水线三阶段接入新客户端

1. `src/pipeline/mod.rs` — 将 `small_model`/`large_model: &'a dyn ChatProvider` 替换为自定义 trait object。
2. `src/pipeline/summary.rs` — 替换 import 与调用，summary 用普通文本响应。
3. `src/pipeline/segment.rs` — 替换 import，请求体带 `response_format: Some(segment_schema())`。
4. `src/pipeline/script.rs` — 替换 import，请求体带 `response_format: Some(script_schema())`。
5. `src/script.rs` 的 `StructuredOutputFormat` 替换为新类型后，调整所有引用点。

### Phase 3：流式接口与服务器侧预留

1. 在 `src/llm/client.rs` 实现 `chat_stream(messages, options)`：
   - 请求体 `stream: true`。
   - 用 `reqwest::Response::bytes_stream()` + `futures_util::StreamExt` 解析 SSE line。
   - 输出 `impl Stream<Item = Result<String, StoryorError>>`（每次 yield 一个 content delta；遇到 `[DONE]` 结束）。
   - 流式响应仅在普通 chat 场景使用，不用于 `json_schema` 严格输出路径。
2. 在 `src/server/types.rs` 新增 Web UI 聊天请求/响应类型（如果当前不存在）。
3. 在 `src/server/routes/mod.rs` 预留 `/api/chat` 路由，用于后续 Web UI 改稿。

### Phase 4：测试 mock 替换

1. `tests/common/mod.rs`
   - 移除 `llm` crate 的所有 import。
   - 改为基于自定义 `ChatClient` trait 实现 `MockProvider`：保留「按调用顺序消费预设响应文本」和「记录 user 消息」的行为。
2. `tests/pipeline_stages.rs` 与 `tests/full_pipeline.rs` — 调整 import 与 test_config 字段。

### Phase 5：依赖与文档清理

1. `Cargo.toml` — 删除 `llm = "1.3.8"` 整行。
2. `Cargo.lock` — 通过 `cargo update` 或重新 `cargo build` 移除 `llm` 子依赖。
3. `PLAN.md` — 删除或替换 llm 相关内容（如 `LLMBuilder::schema()`、`llm` crate 等）。
4. `README.md` — 更新「可用 LLM 服务」说明为「OpenAI 兼容 chat/completions 服务」。
5. `STATUS.md` — 新增条目：移除 `llm` 依赖，新增 `src/llm/` 模块，`chat_stream` 实现。

### Phase 6：验证

1. `cargo check` 全工程无错。
2. `cargo test` 全部通过，尤其：
   - `tests/pipeline_stages.rs`（summary/segment/script 三阶段）
   - `tests/full_pipeline.rs`（checkpoint）
   - `tests/common/mod.rs` 中 mock 测试
   - `tests/export_bindings.rs`（ts-rs 导出 + axfetchum 生成）
3. 手动构造最小配置测试非流式 chat 调用，验证 HTTP 请求体符合 OpenAI schema。

### 关键文件

- `Cargo.toml` — 删除 `llm`，必要时确认 `reqwest`/`futures` 版本。
- `src/error.rs` — 保留/扩展 LLM 错误转换。
- `src/config.rs` — 移除 `parse_backend()` 与 `LLMBackend` 导入。
- `src/script.rs` — 替换 `StructuredOutputFormat` 类型及 schema 函数返回类型。
- `src/llm/mod.rs`、`src/llm/client.rs`、`src/llm/types.rs`（新增）— 自定义客户端核心。
- `src/pipeline/mod.rs`、`src/pipeline/summary.rs`、`src/pipeline/segment.rs`、`src/pipeline/script.rs` — 接入新 `ChatClient`。
- `tests/common/mod.rs`、`tests/pipeline_stages.rs`、`tests/full_pipeline.rs` — mock 替换。
- `PLAN.md`、`README.md`、`STATUS.md` — 文档更新。

### 决策与范围边界

- **包含**：完全移除 `llm` crate；用 `reqwest` 实现非流式 `chat`；新增流式 `chat_stream`；更新流水线、测试、文档。
- **包含**：继续通过 `response_format: {type: "json_schema"}` 获得 `segment`/`script` 的严格 JSON 输出。
- **包含**：`backend` 字段保留当展示/日志，不再解析为 enum。
- **不包含**：Web UI 聊天改稿的完整 UI 与 handler 逻辑（只预留 `chat_stream` 与路由骨架）。
- **不包含**：Ollama/DeepSeek 等方言适配，统一按 OpenAI 兼容接口处理，base_url 可覆盖。
- **不包含**：function/tool calling、`temperature`/`top_p` 等模型参数配置化（后续按需扩展）。

