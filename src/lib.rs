//! storyor —— 小说评书朗读生成器（库入口）
//!
//! 暴露内部模块供集成测试与二进制入口复用。

pub mod audio;
pub mod character;
pub mod checkpoint;
pub mod config;
pub mod error;
pub mod novel;
pub mod pipeline;
pub mod prompts;
pub mod script;
pub mod tts;
