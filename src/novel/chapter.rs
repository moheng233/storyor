//! 正则章节切分
//!
//! 按可配置正则匹配章节标题，将小说切分为 `Vec<Chapter>`。
//! 支持边界情况：无章节标题（整体作为单章）、单章、空内容。

use regex::Regex;

use crate::error::Result;
use crate::script::Chapter;

/// 章节切分器
pub struct ChapterSplitter {
    re: Regex,
}

impl ChapterSplitter {
    /// 用正则字符串构造切分器
    pub fn new(pattern: &str) -> Result<Self> {
        let re = Regex::new(pattern)?;
        Ok(Self { re })
    }

    /// 切分小说文本为章节列表
    ///
    /// 算法：扫描所有匹配的章节标题位置，将相邻标题之间的文本作为
    /// 该章正文。标题之前的引导文本（若有）作为第 0 章的「序言」。
    /// 若无任何匹配，则整体作为单章。
    pub fn split(&self, text: &str) -> Vec<Chapter> {
        let matches: Vec<_> = self.re.find_iter(text).collect();

        if matches.is_empty() {
            // 无章节标题：整体作为单章
            let content = text.trim().to_string();
            return vec![Chapter {
                index: 0,
                title: "全文".to_string(),
                content,
            }];
        }

        let mut chapters = Vec::new();
        let mut last_end = 0usize;

        // 标题之前的引导文本作为「序言」章（若有非空内容）
        if matches[0].start() > 0 {
            let preface = text[..matches[0].start()].trim();
            if !preface.is_empty() {
                chapters.push(Chapter {
                    index: 0,
                    title: "序言".to_string(),
                    content: preface.to_string(),
                });
                last_end = matches[0].start();
            }
        }

        for (i, m) in matches.iter().enumerate() {
            // 标题取匹配所在行的完整文本（含「第1章 开端」这类同行标题）
            let line_start = text[..m.start()].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = text[m.end()..]
                .find('\n')
                .map(|p| m.end() + p)
                .unwrap_or_else(|| {
                    // 末尾无换行，取到下一个匹配起点或文本末尾
                    if i + 1 < matches.len() {
                        matches[i + 1].start()
                    } else {
                        text.len()
                    }
                });
            let title = text[line_start..line_end].trim().to_string();

            // 正文从标题行之后开始
            let body_start = line_end + 1.min(text.len() - line_end);
            let body_end = if i + 1 < matches.len() {
                matches[i + 1].start()
            } else {
                text.len()
            };
            let body_start = body_start.min(body_end);
            let content = text[body_start..body_end].trim().to_string();
            let index = chapters.len();
            chapters.push(Chapter {
                index,
                title,
                content,
            });
            last_end = body_end;
        }

        let _ = last_end; // 保留以备后续使用
        chapters
    }
}

/// 便捷函数：用默认正则切分
pub fn split_default(text: &str) -> Result<Vec<Chapter>> {
    let splitter = ChapterSplitter::new(r"第[零一二三四五六七八九十百千0-9]{1,6}[章节回]")?;
    Ok(splitter.split(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_multiple_chapters() {
        let text = "第1章 开端\n内容一\n第2章 发展\n内容二\n第3章 高潮\n内容三";
        let chapters = split_default(text).unwrap();
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "第1章 开端");
        assert_eq!(chapters[1].title, "第2章 发展");
        assert!(chapters[0].content.contains("内容一"));
    }

    #[test]
    fn split_with_preface() {
        let text = "序言内容\n第1章 开端\n正文";
        let chapters = split_default(text).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "序言");
        assert_eq!(chapters[1].title, "第1章 开端");
    }

    #[test]
    fn split_no_chapter_titles() {
        let text = "这是一段没有章节标题的文本。";
        let chapters = split_default(text).unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "全文");
    }

    #[test]
    fn split_empty_content() {
        let text = "第1章 空\n第2章 有内容\n正文";
        let chapters = split_default(text).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].content, "");
        assert_eq!(chapters[1].content, "正文");
    }

    #[test]
    fn custom_regex_english() {
        let text = "Chapter 1\nA\nChapter 2\nB";
        let splitter = ChapterSplitter::new(r"Chapter \d+").unwrap();
        let chapters = splitter.split(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter 1");
    }
}
