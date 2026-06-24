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
5. 如果只有一个段落，也必须返回 `paragraphs: [{...}]`，不能把 `handoff` 混入 `paragraphs`
6. 情绪、动作、语速等提示必须直接写进 `content`，不要输出单独的 `tags` 字段
