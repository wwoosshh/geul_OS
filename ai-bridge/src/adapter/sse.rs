//! Anthropic messages SSE 증분 파서.
//!
//! 네트워크/async 무관 — 바이트 청크를 push하면 완성된 SSE 이벤트를 뱉는다.
//! SSE 프레임 경계는 빈 줄(`\n\n`). 한 프레임은 `event: <name>` + `data: <json>` 라인.

use serde_json::Value;

/// 파싱된 SSE 이벤트 (필요한 것만; 나머지는 Other).
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    MessageStart,
    /// content block 시작 — Text 또는 ToolUse.
    ContentBlockStart {
        index: usize,
        tool_name: Option<String>,
    },
    /// text_delta 토막.
    TextDelta {
        index: usize,
        text: String,
    },
    /// message_delta — stop_reason + output_tokens.
    MessageDelta {
        stop_reason: Option<String>,
        output_tokens: u64,
    },
    MessageStop,
    /// ping / input_json_delta / content_block_stop 등 무시 대상.
    Other,
}

/// 증분 SSE 파서 — push로 청크를 먹이고 완성된 이벤트를 받는다.
#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 바이트 청크를 누적하고, 지금까지 완성된 프레임(`\n\n` 종결)을 파싱해 반환.
    /// 미완 프레임은 내부 buf에 보존.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        while let Some(idx) = self.buf.find("\n\n") {
            let frame: String = self.buf.drain(..idx + 2).collect();
            if let Some(ev) = parse_frame(&frame) {
                events.push(ev);
            }
        }
        events
    }
}

/// 한 SSE 프레임("event: ...\ndata: ...\n\n")을 SseEvent로. data JSON의 type으로 분기.
fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut data_json: Option<Value> = None;
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_json = serde_json::from_str(rest.trim()).ok();
        }
    }
    let data = data_json?;
    let ty = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ty {
        "message_start" => Some(SseEvent::MessageStart),
        "content_block_start" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tool_name = data
                .get("content_block")
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                .and_then(|b| b.get("name").and_then(|v| v.as_str()))
                .map(String::from);
            Some(SseEvent::ContentBlockStart { index, tool_name })
        }
        "content_block_delta" => {
            let delta = data.get("delta")?;
            if delta.get("type").and_then(|v| v.as_str()) == Some("text_delta") {
                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                Some(SseEvent::TextDelta { index, text })
            } else {
                Some(SseEvent::Other)
            }
        }
        "message_delta" => {
            let stop_reason = data
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let output_tokens = data
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(SseEvent::MessageDelta { stop_reason, output_tokens })
        }
        "message_stop" => Some(SseEvent::MessageStop),
        _ => Some(SseEvent::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_deltas_in_order() {
        let mut p = SseParser::new();
        let sse = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"안녕\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" 세계\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let evs = p.push(sse.as_bytes());
        assert_eq!(evs[0], SseEvent::MessageStart);
        assert!(matches!(evs[1], SseEvent::ContentBlockStart { index: 0, tool_name: None }));
        assert_eq!(evs[2], SseEvent::TextDelta { index: 0, text: "안녕".into() });
        assert_eq!(evs[3], SseEvent::TextDelta { index: 0, text: " 세계".into() });
        assert_eq!(
            evs[4],
            SseEvent::MessageDelta { stop_reason: Some("end_turn".into()), output_tokens: 7 }
        );
        assert_eq!(evs[5], SseEvent::MessageStop);
    }

    #[test]
    fn reassembles_frame_split_across_chunks() {
        let mut p = SseParser::new();
        let e1 = p.push(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,",
        );
        assert!(e1.is_empty(), "미완 프레임은 이벤트 0");
        let e2 = p.push(b"\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n");
        assert_eq!(e2, vec![SseEvent::TextDelta { index: 0, text: "x".into() }]);
    }

    #[test]
    fn tool_use_block_start_carries_name() {
        let mut p = SseParser::new();
        let f = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"get_object\"}}\n\n";
        let evs = p.push(f.as_bytes());
        assert_eq!(
            evs,
            vec![SseEvent::ContentBlockStart { index: 1, tool_name: Some("get_object".into()) }]
        );
    }
}
