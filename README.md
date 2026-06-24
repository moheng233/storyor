# 说书人 (storyor)

> 将小说转化为评书风格有声读物的 AI 驱动流水线。

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## 项目简介

**说书人** 是一个基于大语言模型（LLM）和语音合成（TTS）的自动化工具，能将纯文本小说转化为富有表现力的评书风格音频。它不仅仅是机械的文本转语音——而是像一位经验丰富的评书艺人和导演，分析剧情脉络、设计角色音色、编排台词节奏，最终合成出具有角色一致性和情感张力的有声作品。

## 当前状态

| 版本 | 状态 | 说明 |
|------|------|------|
| **v1** | ✅ 已完成 | CLI 单次流水线，全流程验证通过（章节切分 → 摘要 → 段划分 → 剧本生成 → 音频合成） |
| **v2** | 📋 规划中 | 交互式 Web UI 工作流重构，四阶段独立可干预 |

详见 [STATUS.md](./STATUS.md) 和 [PLAN.md](./PLAN.md)。

## v1 核心能力（已验证）

### 流水线阶段

```
小说文本 → 章节切分 → 章节摘要 → 剧情段划分 → 剧本生成 → 音频合成
```

1. **章节切分** — 正则匹配章节标题，自动拆分
2. **章节摘要** — 小模型并发生成每章 1-2 句摘要
3. **剧情段划分** — 大模型按剧情弧线切分为独立叙事单元
4. **剧本生成** — 每段输出角色库 + 台词 + 段落分组 + 衔接话，采用导演模式描述
5. **音频合成** — TTS 模型逐句合成，自动合并为段级 WAV

### 关键特性

- 🎭 **导演模式** — 从角色、场景、指导三个维度刻画每句台词的演绎要领
- 🎤 **内联情感标注** — 台词内容直接包含 `（怅然）` `（深呼吸）` 等演绎指令
- 📚 **角色库一致性** — 全流程维护统一角色档案，跨段传递 handoff 衔接上下文
- 💾 **断点续跑** — checkpoint.json 驱动，支持段/句粒度恢复
- 🧪 **24 个测试** — 含 mock LLM provider 的集成测试覆盖全流程

## v2 架构展望

将 v1 的单次 CLI 流水线升级为 **交互式 Web 应用**，用户可以分阶段审核和修改 AI 生成结果：

| 阶段 | 内容 | 用户操作 |
|------|------|----------|
| 预处理 | 章节切分、摘要、段划分 | 编辑修正 |
| 剧本生成 | 角色库、台词、衔接话 | 三种修改模式：重新生成 / 对话修改 / 直接编辑 |
| 音色设计 | 按角色生成参考音频 | 试听 → 修改 guidance → 重新生成 → 确认 |
| 音频合成 | 逐段合成最终音频 | 按段独立合成、单句重新生成 |

### TTS 音色工作流（v2 核心亮点）

传统方案每次合成时通过文字描述指定音色，无法保证跨对话一致性。说书人 v2 将音色固定为两阶段：

1. **音色设计** — 调用 `voicedesign` 模型，按角色 guidance 生成一段参考音频
2. **人工确认** — 试听参考音频，不满意可修改 guidance 重新生成，直到满意后锁定
3. **音色复刻** — 调用 `voiceclone` 模型，以确认的参考音频为模板，逐句合成该角色的所有台词，确保全剧音色一致

> 以上工作流依赖 TTS 服务原生支持 `voicedesign` / `voiceclone` 能力。推荐使用 **mimo-v2.5-tts 系列**模型。

### v2 技术栈

| 层 | 技术 |
|----|------|
| 后端 | Rust + axum + SSE 进度推送 |
| 前端 | React + TypeScript + [shadcn/ui](https://ui.shadcn.com/) + Tailwind CSS |
| 类型桥接 | `ts-rs`（Rust → TS 类型导出）+ `axfetchum`（路由 → TS API 客户端生成） |
| 数据存储 | 文件系统（JSON + 目录），每个项目独立 workspace |

## 快速开始

### 前置要求

- Rust 工具链（2024 edition）
- 可用的 LLM 服务（OpenAI 兼容接口）
- **TTS 服务须支持音色设计与复刻功能**：
  - `voicedesign` — 根据文字描述生成角色参考音色（阶段 3 音色设计）
  - `voiceclone`  — 根据参考音频 + 目标文本复刻音色进行合成（阶段 4 音频合成）
  - 以上功能非标准 OpenAI TTS 接口，需 TTS 服务提供方原生支持
  - 🔧 目前推荐使用 **mimo-v2.5-tts 系列**：
    - `mimo-v2.5-tts-voicedesign` — 音色设计模型
    - `mimo-v2.5-tts-voiceclone`  — 音色复刻模型

### 安装

```bash
git clone <repo-url>
cd storyor
cargo build --release
```

### 配置

复制示例配置文件并按需修改：

```bash
cp storyor.example.toml storyor.toml
```

配置文件包含小模型、大模型以及两组 TTS 模型（音色设计 + 音色复刻）的连接信息。

```toml
# 音色设计模型：根据 guidance 文本生成参考音频
[voice_design_model]
backend = "OpenAI"
model = "mimo-v2.5-tts-voicedesign"
# ...

# 音色复刻模型：根据参考音频 + 台词合成最终音频
[voice_clone_model]
backend = "OpenAI"
model = "mimo-v2.5-tts-voiceclone"
# ...
```

> ⚠️ **重要**：TTS 模型必须原生支持 `voicedesign` 和 `voiceclone` 能力。这并非标准 OpenAI `audio/transcriptions` 或 `audio/speech` 接口。推荐使用 **mimo-v2.5-tts 系列**模型，其他 TTS 服务需自行确认兼容性。

### 运行（v1 CLI 模式）

```bash
cargo run -- --input <小说文件> --config storyor.toml --output ./output
```

产物输出到指定目录，包含章节、摘要、剧本 JSON、角色库、分段音频和清单文件。详见 [产物目录结构](./PLAN.md#项目产物目录结构每个项目独立)。

## 项目结构

```
src/
  main.rs              CLI 入口
  config.rs            配置管理
  error.rs             错误类型
  novel/chapter.rs     章节切分
  pipeline/            流水线编排
    mod.rs             串联 + 断点续跑
    summary.rs         章节摘要
    segment.rs         剧情段切分
    script.rs          剧本生成
  character.rs         角色库管理
  script.rs            数据结构定义
  tts/client.rs        TTS 客户端
  audio.rs             音频落盘 + 合并
  checkpoint.rs        产物管理 + 续跑
prompts/               提示词模板
  story_teller.md      说书人风格系统提示词
  summary.md / segment.md / script.md
tests/                 24 个测试（单元 + 集成）
```

## 开发计划

参见 [PLAN.md](./PLAN.md) 获取 v2 完整架构设计和实施步骤。概要：

- **Phase A** — 后端基础设施（axum server + 项目管理）
- **Phase B** — 流水线解耦 + TTS 双模式（voicedesign / voiceclone）
- **Phase C** — REST API 端点（项目管理 / 预处理 / 剧本 / 音色 / 音频）
- **Phase D** — React 前端（shadcn/ui + TypeScript）
- **Phase E** — 配置扩展 + CLI 适配 + 构建整合

## License

MIT
