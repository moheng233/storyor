//! 角色库管理 + 合并更新
//!
//! 角色库顺序生成：每段剧本生成时注入「上段 handoff + 当前角色库」，
//! 模型输出更新后的角色库，通过 `merge` 合并新增/更新已有角色。

use crate::script::{CharacterLibrary, CharacterProfile};

impl CharacterLibrary {
    /// 创建空角色库
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
        }
    }

    /// 合并新角色：新角色加入，已有角色按段更新（保留最新 guidance）
    pub fn merge(&mut self, new_chars: &[CharacterProfile]) {
        for ch in new_chars {
            self.characters.insert(ch.name.clone(), ch.clone());
        }
    }

    /// 合并另一个角色库
    pub fn merge_library(&mut self, other: &CharacterLibrary) {
        for (name, profile) in &other.characters {
            self.characters.insert(name.clone(), profile.clone());
        }
    }

    /// 获取角色档案
    pub fn get(&self, name: &str) -> Option<&CharacterProfile> {
        self.characters.get(name)
    }

    /// 角色数量
    pub fn len(&self) -> usize {
        self.characters.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty()
    }

    /// 渲染为可注入 prompt 的文本（角色名 + guidance）
    pub fn render_for_prompt(&self) -> String {
        if self.characters.is_empty() {
            return "（暂无角色）".to_string();
        }
        let mut entries: Vec<_> = self.characters.values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
            .iter()
            .map(|ch| {
                format!(
                    "- {name}：{profile}\n  场景：{scene}\n  音色：{guidance}",
                    name = ch.name,
                    profile = ch.profile,
                    scene = ch.scene,
                    guidance = ch.guidance
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, guidance: &str) -> CharacterProfile {
        CharacterProfile {
            name: name.to_string(),
            profile: "测试".to_string(),
            scene: "测试场景".to_string(),
            guidance: guidance.to_string(),
        }
    }

    #[test]
    fn merge_adds_new_character() {
        let mut lib = CharacterLibrary::new();
        lib.merge(&[profile("张三", "低沉")]);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib.get("张三").unwrap().guidance, "低沉");
    }

    #[test]
    fn merge_updates_existing() {
        let mut lib = CharacterLibrary::new();
        lib.merge(&[profile("张三", "低沉")]);
        lib.merge(&[profile("张三", "激昂")]);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib.get("张三").unwrap().guidance, "激昂");
    }

    #[test]
    fn merge_library_combines() {
        let mut lib = CharacterLibrary::new();
        lib.merge(&[profile("张三", "低沉")]);
        let mut other = CharacterLibrary::new();
        other.merge(&[profile("李四", "清亮")]);
        lib.merge_library(&other);
        assert_eq!(lib.len(), 2);
    }
}
