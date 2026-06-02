//! Anthropic messages SSE 증분 파서.
//!
//! 네트워크/async 무관 — 바이트 청크를 push하면 완성된 SSE 이벤트를 뱉는다.
//! SSE 프레임 경계는 빈 줄(`\n\n`). 한 프레임은 `event: <name>` + `data: <json>` 라인.

use serde_json::Value;

/// 파싱된 SSE 이벤트 (필요한 것만; 나머지는 Other).
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    MessageStart,
    /// content block 시작 — Text 또는 ToolUse (tool_use면 tool_name/tool_id 채움).
    ContentBlockStart {
        index: usize,
        tool_name: Option<String>,
        tool_id: Option<String>,
    },
    /// text_delta 토막.
    TextDelta {
        index: usize,
        text: String,
    },
    /// input_json_delta 토막 — tool_use 인자 JSON의 부분 문자열. index별로 누적해야 완성.
    InputJsonDelta {
        index: usize,
        partial_json: String,
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
///
/// **버퍼는 바이트(Vec<u8>)로 누적**한다. 청크마다 `from_utf8_lossy`를 호출하면 멀티바이트
/// UTF-8 문자(한글 등)가 청크 경계에 걸릴 때 replacement char로 손상돼 도구 인자 JSON이
/// 깨진다(2026-06-02 진단). 완성된 프레임(`\n\n` 경계)만 디코딩하면 프레임 내부는 항상
/// 완전한 UTF-8이라 안전.
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 바이트 청크를 누적하고, 지금까지 완성된 프레임(`\n\n` 종결)을 파싱해 반환.
    /// 미완 프레임은 내부 buf에 보존.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(idx) = self.buf.windows(2).position(|w| w == b"\n\n") {
            let frame: Vec<u8> = self.buf.drain(..idx + 2).collect();
            // 완성된 프레임이므로 내부는 완전한 UTF-8 — lossy 디코딩이 안전.
            if let Some(ev) = parse_frame(&String::from_utf8_lossy(&frame)) {
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
            match serde_json::from_str(rest.trim()) {
                Ok(v) => data_json = Some(v),
                Err(e) => eprintln!(
                    "[sse-trace] data 파싱 실패: {} | line_len={} head={:.100}",
                    e,
                    rest.trim().len(),
                    rest.trim()
                ),
            }
        }
    }
    let data = data_json?;
    let ty = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ty {
        "message_start" => Some(SseEvent::MessageStart),
        "content_block_start" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tool_block = data
                .get("content_block")
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"));
            let tool_name =
                tool_block.and_then(|b| b.get("name").and_then(|v| v.as_str())).map(String::from);
            let tool_id =
                tool_block.and_then(|b| b.get("id").and_then(|v| v.as_str())).map(String::from);
            Some(SseEvent::ContentBlockStart { index, tool_name, tool_id })
        }
        "content_block_delta" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let delta = data.get("delta")?;
            match delta.get("type").and_then(|v| v.as_str()) {
                Some("text_delta") => {
                    let text =
                        delta.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some(SseEvent::TextDelta { index, text })
                }
                Some("input_json_delta") => {
                    let partial_json = delta
                        .get("partial_json")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(SseEvent::InputJsonDelta { index, partial_json })
                }
                _ => Some(SseEvent::Other),
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
        assert!(matches!(
            evs[1],
            SseEvent::ContentBlockStart { index: 0, tool_name: None, tool_id: None }
        ));
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
    fn tool_use_block_start_carries_name_and_id() {
        let mut p = SseParser::new();
        let f = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"get_object\"}}\n\n";
        let evs = p.push(f.as_bytes());
        assert_eq!(
            evs,
            vec![SseEvent::ContentBlockStart {
                index: 1,
                tool_name: Some("get_object".into()),
                tool_id: Some("t1".into()),
            }]
        );
    }

    #[test]
    fn input_json_delta_fragments_emitted_for_accumulation() {
        // tool_use 인자는 input_json_delta 토막으로 도착 — caller가 index별 누적 후 파싱.
        let mut p = SseParser::new();
        let sse = concat!(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"target\\\":\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"abc\\\"}\"}}\n\n",
        );
        let evs = p.push(sse.as_bytes());
        assert_eq!(
            evs,
            vec![
                SseEvent::InputJsonDelta { index: 1, partial_json: "{\"target\":".into() },
                SseEvent::InputJsonDelta { index: 1, partial_json: "\"abc\"}".into() },
            ]
        );
        // 누적하면 valid JSON.
        let joined = "{\"target\":\"abc\"}";
        let v: serde_json::Value = serde_json::from_str(joined).unwrap();
        assert_eq!(v.get("target").and_then(|x| x.as_str()), Some("abc"));
    }
}
