请根据以下信息生成本剧情段的评书剧本。

## 本段剧情概述
{{segment_summary}}

## 上段衔接话
{{prev_handoff}}

## 当前角色库
{{characters}}

## 本段章节原文
{{chapter_text}}

## 要求
1. 生成评书风格剧本，旁白用评书口吻
2. 更新角色库：新增本段出现的角色，已有角色可更新音色设定
3. 按情绪/场景转变划分段落，每段不超过 {{max_paragraph_lines}} 行台词
4. 末尾给出 handoff：传递给下一段的剧情衔接说明

## 输出格式
严格输出 JSON：
```json
{
  "characters": [{"name":"角色名","profile":"简介","scene":"场景","guidance":"TTS音色设定"}],
  "paragraphs": [{"index":0,"lines":[{"speaker":"旁白","content":"台词","tags":["情绪标签"]}]}],
  "handoff": "下一段剧情衔接说明"
}
```
