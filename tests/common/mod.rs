//! 集成测试公共工具：mock LLM provider 与测试数据

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use storyor::error::StoryorError;
use storyor::llm::{ChatClient, ChatDeltaStream, ChatMessage, ChatResponse, JsonSchemaFormat};

// ---------------------------------------------------------------------------
// Mock ChatResponse（直接复用 storyor::llm::ChatResponse，构造固定文本）
// ---------------------------------------------------------------------------

/// 构造一个仅含文本内容的 `ChatResponse`（mock 用）
pub fn mock_response(text: impl Into<String>) -> ChatResponse {
    ChatResponse {
        id: "mock".to_string(),
        model: "mock-model".to_string(),
        choices: vec![storyor::llm::Choice {
            index: 0,
            message: storyor::llm::ResponseMessage {
                role: "assistant".to_string(),
                content: Some(text.into()),
                audio: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    }
}

// ---------------------------------------------------------------------------
// Mock ChatClient
// ---------------------------------------------------------------------------

/// 按调用顺序返回预设响应的 mock provider
///
/// 内部维护一个响应队列，每次 `chat_with_schema()` 调用弹出下一个响应。
/// 同时记录所有调用收到的消息内容，便于断言。
pub struct MockProvider {
    /// 预设响应队列（按调用顺序消费）
    responses: Arc<Mutex<Vec<String>>>,
    /// 记录每次调用收到的 user 消息内容
    pub received_user_contents: Arc<Mutex<Vec<String>>>,
}

impl MockProvider {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            received_user_contents: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 记录收到的 user 消息内容（用于断言 prompt 构造正确性）
    fn record(&self, messages: &[ChatMessage]) {
        let user_text: Vec<String> = messages
            .iter()
            .filter(|m| m.is_user())
            .map(|m| m.content.clone())
            .collect();
        self.received_user_contents
            .lock()
            .unwrap()
            .extend(user_text);
    }

    /// 弹出下一个预设响应文本
    fn next_response(&self) -> Result<String, StoryorError> {
        let mut queue = self.responses.lock().unwrap();
        if queue.is_empty() {
            return Err(StoryorError::Llm(
                "mock provider 响应队列已耗尽".to_string(),
            ));
        }
        Ok(queue.remove(0))
    }
}

#[async_trait]
impl ChatClient for MockProvider {
    async fn chat_with_schema(
        &self,
        messages: &[ChatMessage],
        _schema: Option<&JsonSchemaFormat>,
    ) -> Result<ChatResponse, StoryorError> {
        self.record(messages);
        let text = self.next_response()?;
        Ok(mock_response(text))
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<ChatResponse, StoryorError> {
        self.chat_with_schema(messages, None).await
    }

    fn chat_stream(&self, messages: Vec<ChatMessage>) -> ChatDeltaStream<'_> {
        self.record(&messages);
        // 把队列中下一个响应整体作为一个 delta 一次性产出。
        let next = self.next_response();
        let stream = async_stream::stream! {
            match next {
                Ok(text) => yield Ok(text),
                Err(e) => yield Err(e),
            }
        };
        Box::pin(stream)
    }
}

// ---------------------------------------------------------------------------
// 测试数据
// ---------------------------------------------------------------------------

/// 构造一份小型测试小说（3 章）
pub fn sample_novel() -> String {
    "第1章 初遇\n少年林风初入江湖，于山道偶遇白衣女子，二人结伴同行。\n\
     第2章 风波\n客栈中突遇黑衣人袭击，林风拔剑相护，女子展露惊人武功。\n\
     第3章 真相\n女子自陈身份乃前朝公主，林风决意护送其入京。"
        .to_string()
}

/// 章节摘要的 mock 响应（3 章）
pub fn mock_summary_responses() -> Vec<String> {
    vec![
        "林风初入江湖，于山道偶遇白衣女子，结伴同行。".to_string(),
        "客栈遇黑衣人袭击，林风拔剑相护，女子展露惊人武功。".to_string(),
        "女子自陈前朝公主身份，林风决意护送入京。".to_string(),
    ]
}

/// 剧情段切分的 mock 响应（2 段）
pub fn mock_segment_response() -> String {
    serde_json::json!({
        "segments": [
            {"index": 0, "chapter_start": 0, "chapter_end": 1, "summary": "林风初遇白衣女子，客栈遭袭"},
            {"index": 1, "chapter_start": 2, "chapter_end": 2, "summary": "女子身份揭晓，林风护送入京"}
        ]
    })
    .to_string()
}

/// 剧本生成的 mock 响应（剧情段 0）
pub fn mock_script_response_0() -> String {
    serde_json::json!({
        "characters": [
            {"name": "林风", "profile": "少年剑客", "scene": "山道、客栈", "guidance": "青年男声，清朗坚定"},
            {"name": "白衣女子", "profile": "前朝公主", "scene": "山道、客栈", "guidance": "青年女声，清冷沉稳"}
        ],
        "paragraphs": [
            {
                "index": 0,
                "lines": [
                    {"speaker": "旁白", "content": "（舒缓，娓娓道来）话说林风初入江湖，行至山道，忽见一白衣女子立于道旁。", "description": "角色：评书艺人，沉稳洪亮的中年男性。\n场景：开篇，面对听众讲述林风初入江湖的见闻，营造山道偶遇的悬念感。\n指导：语速舒缓，娓娓道来，句末微微上扬，留出悬念的气口。"},
                    {"speaker": "林风", "content": "（关切）姑娘独行山路，可需相伴？", "description": "角色：少年剑客林风，热血赤诚，初出茅庐。\n场景：山道偶遇白衣女子，出于侠义之心主动搭话，语气关切但不轻浮。\n指导：青年男声，清朗干净。语速适中，语调上扬以示关切，尾音略收以表礼貌。"}
                ]
            },
            {
                "index": 1,
                "lines": [
                    {"speaker": "白衣女子", "content": "（淡然）多谢公子好意，小女子自可行走。", "description": "角色：白衣女子，前朝公主，清冷沉稳。\n场景：山道，婉拒陌生少年的搭话，表面淡然但心中警觉。\n指导：青年女声，清冷疏离。语速略慢，语气平淡不露情绪，尾音微微下垂以示距离感。"}
                ]
            }
        ],
        "handoff": "二人结伴同行，夜宿客栈，暗流涌动。"
    })
    .to_string()
}

/// 剧本生成的 mock 响应（剧情段 1）
pub fn mock_script_response_1() -> String {
    serde_json::json!({
        "characters": [
            {"name": "林风", "profile": "少年剑客", "scene": "山道、客栈、入京路", "guidance": "青年男声，清朗坚定"},
            {"name": "白衣女子", "profile": "前朝公主，名萧云", "scene": "山道、客栈、入京路", "guidance": "青年女声，清冷沉稳，偶露威严"}
        ],
        "paragraphs": [
            {
                "index": 0,
                "lines": [
                    {"speaker": "白衣女子", "content": "（郑重）林公子，我本前朝公主萧云，此番入京，乃为复国大计。", "description": "角色：白衣女子萧云，前朝公主，外表清冷内心坚毅。\n场景：客栈中，向林风坦白真实身份，语气郑重，带着托付命运的信任与决绝。\n指导：青年女声，从清冷渐变沉稳郑重。起句略低，到'复国大计'四字加重，胸腔共鸣增强，眼神坚定。"},
                    {"speaker": "林风", "content": "（坚定）既已同行至此，风愿护送公主入京。", "description": "角色：少年剑客林风，义薄云天，一诺千金。\n场景：客栈中听完萧云自白，毫不犹豫许下承诺，语气坚定无一丝动摇。\n指导：青年男声，语气坚定有力，语速利落不拖沓，尾音落得干净果断。"}
                ]
            }
        ],
        "handoff": "二人踏上入京之路，前路未卜。"
    })
    .to_string()
}
