//! 원격 TCP transport + RemoteShell.
//!
//! `RemoteTransport`는 server-host 에 연결하여 Hello/HelloAck 핸드셰이크를 완료하고,
//! 이후 한 request / 한 response 방식으로 wire 메시지를 주고받는다.
//!
//! `RemoteShell`은 `RemoteTransport`를 감싸고 mount/invoke/ls 세 명령을 구현한다.
//! (M2 스코프: mount + invoke + ls 만 원격 지원. 나머지는 이후 PR.)

use std::collections::HashMap;

use geulos_core::{std_types, ActorId};
use geulos_proto::{
    decode_frame, encode_frame, Hello, HelloAck, InvokeAck, InvokeError, InvokeMsg, MountAck,
    MountMsg, QueryMsg, QueryPredicate, QueryResult, Role,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ─── RemoteTransport ──────────────────────────────────────────────────────────

/// 원격 transport. tokio runtime 안에서만 사용.
pub struct RemoteTransport {
    stream: TcpStream,
    /// 서버가 발급한 actor_id (HelloAck).
    pub actor_id: String,
    /// 아직 소비되지 않은 수신 바이트.
    accum: Vec<u8>,
}

impl RemoteTransport {
    /// 접속 + Hello/HelloAck 핸드셰이크.
    pub async fn connect(addr: &str, role: Role) -> Result<Self, String> {
        let mut stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;

        let hello = Hello {
            version: "0.1".to_string(),
            role,
            auth: serde_json::json!({}),
            client_id: "geulosh".to_string(),
        };
        let body = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
        stream.write_all(&encode_frame(&body)).await.map_err(|e| e.to_string())?;

        // HelloAck (또는 HelloReject) 수신
        let mut accum: Vec<u8> = Vec::new();
        let mut tmp = vec![0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("연결이 HelloAck 전에 닫힘".to_string());
            }
            accum.extend_from_slice(&tmp[..n]);

            let mut slice = accum.as_slice();
            match decode_frame(&mut slice) {
                Ok(frame_body) => {
                    let consumed = accum.len() - slice.len();
                    accum.drain(..consumed);

                    // HelloAck? 먼저 시도
                    if let Ok(ack) = serde_json::from_slice::<HelloAck>(&frame_body) {
                        return Ok(Self { stream, actor_id: ack.actor_id, accum });
                    }
                    // HelloReject?
                    let text = String::from_utf8_lossy(&frame_body);
                    return Err(format!("핸드셰이크 실패: {}", text));
                }
                Err(_) => continue, // 더 읽어야 함
            }
        }
    }

    /// 한 메시지를 보내고 한 응답 프레임을 받는다.
    pub async fn request(&mut self, body: &[u8]) -> Result<Vec<u8>, String> {
        self.stream.write_all(&encode_frame(body)).await.map_err(|e| e.to_string())?;

        let mut tmp = vec![0u8; 4096];
        loop {
            // 이미 accum에 응답이 있을 수 있음
            {
                let mut slice = self.accum.as_slice();
                if let Ok(resp) = decode_frame(&mut slice) {
                    let consumed = self.accum.len() - slice.len();
                    self.accum.drain(..consumed);
                    return Ok(resp);
                }
            }
            let n = self.stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("연결이 응답 전에 닫힘".to_string());
            }
            self.accum.extend_from_slice(&tmp[..n]);
        }
    }
}

// ─── RemoteShell ─────────────────────────────────────────────────────────────

/// 원격 셸 상태. `RemoteTransport`를 감싸고 label 맵을 유지.
pub struct RemoteShell {
    pub transport: RemoteTransport,
    /// 짧은 라벨 (`#N`) → ObjectId 문자열.
    labels: HashMap<u32, String>,
    next_label: u32,
    /// 다음 request_id 카운터.
    next_req: u32,
}

impl RemoteShell {
    pub fn new(transport: RemoteTransport) -> Self {
        Self { transport, labels: HashMap::new(), next_label: 1, next_req: 1 }
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("r-{}", self.next_req);
        self.next_req += 1;
        id
    }

    fn assign_label(&mut self, object_id: String) -> u32 {
        let n = self.next_label;
        self.labels.insert(n, object_id);
        self.next_label += 1;
        n
    }

    fn resolve_label(&self, tok: &str) -> Result<String, String> {
        if let Some(n_str) = tok.strip_prefix('#') {
            let n: u32 = n_str.parse().map_err(|_| format!("잘못된 라벨: {}", tok))?;
            self.labels.get(&n).cloned().ok_or_else(|| format!("라벨 없음: {} — ls 확인", tok))
        } else {
            // UUID 직접 사용
            Ok(tok.to_string())
        }
    }

    // ── 공개 명령 ──

    /// `mount text <content>` — 서버 측 ObjectServer에 Text 객체를 마운트.
    pub async fn mount_text(&mut self, content: &str) -> Result<String, String> {
        let obj = std_types::text(ActorId::local_user(), content);
        let type_uri = obj.type_uri.as_str().to_string();
        self.mount_object(obj, &type_uri).await
    }

    /// `mount button <label>` — 서버 측 ObjectServer에 Button 객체를 마운트.
    pub async fn mount_button(&mut self, label_text: &str) -> Result<String, String> {
        let obj = std_types::button(ActorId::local_user(), label_text);
        let type_uri = obj.type_uri.as_str().to_string();
        self.mount_object(obj, &type_uri).await
    }

    /// 내부: Object를 직렬화해 MountMsg를 전송하고 MountAck/MountReject를 처리.
    async fn mount_object(
        &mut self,
        obj: geulos_core::Object,
        type_uri: &str,
    ) -> Result<String, String> {
        let obj_id = obj.id.to_string();
        let tree = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
        let msg = MountMsg { root_object_id: obj_id.clone(), tree };
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        let resp = self.transport.request(&body).await?;

        let resp_val: Value = serde_json::from_slice(&resp).map_err(|e| e.to_string())?;
        let kind = resp_val.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "MountAck" => {
                let ack: MountAck = serde_json::from_value(resp_val).map_err(|e| e.to_string())?;
                let n = self.assign_label(ack.root_object_id.clone());
                Ok(format!("Created #{} ({})", n, type_uri))
            }
            "MountReject" => {
                let reason = resp_val.get("reason").and_then(Value::as_str).unwrap_or("unknown");
                let detail = resp_val.get("detail").and_then(Value::as_str).unwrap_or("");
                Err(format!("MountReject: {} — {}", reason, detail))
            }
            _ => Err(format!("예상치 못한 응답: {}", String::from_utf8_lossy(&resp))),
        }
    }

    /// `invoke #N <method>` — 서버 측 Invoke.
    pub async fn invoke(&mut self, target_tok: &str, method: &str) -> Result<String, String> {
        let object_id = self.resolve_label(target_tok)?;
        let req_id = self.next_request_id();
        let msg = InvokeMsg {
            request_id: req_id.clone(),
            target: object_id,
            method: method.to_string(),
            args: Value::Null,
        };
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        let resp = self.transport.request(&body).await?;

        let resp_val: Value = serde_json::from_slice(&resp).map_err(|e| e.to_string())?;
        let kind = resp_val.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "InvokeAck" => {
                let ack: InvokeAck = serde_json::from_value(resp_val).map_err(|e| e.to_string())?;
                Ok(format!("Invoke event {} emitted (req {})", ack.event_id, ack.request_id))
            }
            "InvokeError" => {
                let err: InvokeError =
                    serde_json::from_value(resp_val).map_err(|e| e.to_string())?;
                Err(format!("InvokeError[{}]: {}", err.kind, err.detail))
            }
            _ => Err(format!("예상치 못한 응답: {}", String::from_utf8_lossy(&resp))),
        }
    }

    /// `ls` — 서버 측 Query(ByOwner user:local)로 전체 객체 목록.
    pub async fn ls(&mut self) -> Result<String, String> {
        let req_id = self.next_request_id();
        let msg = QueryMsg {
            request_id: req_id,
            query: QueryPredicate::ByOwner { actor: "user:local".to_string() },
        };
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        let resp = self.transport.request(&body).await?;

        let resp_val: Value = serde_json::from_slice(&resp).map_err(|e| e.to_string())?;
        let kind = resp_val.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind == "QueryResult" {
            let result: QueryResult =
                serde_json::from_value(resp_val).map_err(|e| e.to_string())?;
            if result.objects.is_empty() {
                return Ok("(no objects)".to_string());
            }
            let lines: Vec<String> = result
                .objects
                .iter()
                .map(|id| {
                    // 라벨 역조회 (있으면 표시)
                    let label = self.labels.iter().find(|(_, v)| *v == id).map(|(n, _)| *n);
                    match label {
                        Some(n) => format!("#{} {}", n, id),
                        None => format!("    {}", id),
                    }
                })
                .collect();
            Ok(lines.join("\n"))
        } else {
            Err(format!("예상치 못한 응답: {}", String::from_utf8_lossy(&resp)))
        }
    }

    /// 한 줄 명령을 파싱하고 실행한다.
    /// 지원 명령: mount text/button, invoke, ls, exit/quit, help
    pub async fn execute(&mut self, line: &str) -> RemoteOutcome {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return RemoteOutcome::NoOp;
        }
        let toks = tokenize_simple(trimmed);
        if toks.is_empty() {
            return RemoteOutcome::NoOp;
        }
        match toks[0].as_str() {
            "exit" | "quit" => RemoteOutcome::Quit,
            "help" => RemoteOutcome::Output(REMOTE_HELP.to_string()),
            "ls" => match self.ls().await {
                Ok(s) => RemoteOutcome::Output(s),
                Err(e) => RemoteOutcome::Error(e),
            },
            "mount" => {
                let kind = toks.get(1).map(String::as_str).unwrap_or("");
                match kind {
                    "text" => {
                        let content = toks.get(2).map(String::as_str).unwrap_or("");
                        match self.mount_text(content).await {
                            Ok(s) => RemoteOutcome::Output(s),
                            Err(e) => RemoteOutcome::Error(e),
                        }
                    }
                    "button" => {
                        let label = toks.get(2).map(String::as_str).unwrap_or("");
                        match self.mount_button(label).await {
                            Ok(s) => RemoteOutcome::Output(s),
                            Err(e) => RemoteOutcome::Error(e),
                        }
                    }
                    _ => {
                        RemoteOutcome::Error("remote mode: mount text|button <content>".to_string())
                    }
                }
            }
            "invoke" => {
                let target = match toks.get(1) {
                    Some(t) => t.as_str(),
                    None => {
                        return RemoteOutcome::Error("invoke #N <method>".to_string());
                    }
                };
                let method = match toks.get(2) {
                    Some(m) => m.as_str(),
                    None => {
                        return RemoteOutcome::Error("invoke #N <method>".to_string());
                    }
                };
                match self.invoke(target, method).await {
                    Ok(s) => RemoteOutcome::Output(s),
                    Err(e) => RemoteOutcome::Error(e),
                }
            }
            cmd => {
                RemoteOutcome::Error(format!("remote mode: 알 수 없는 명령 '{}' — help 참고", cmd))
            }
        }
    }
}

/// 원격 셸 명령 실행 결과.
#[derive(Debug)]
pub enum RemoteOutcome {
    Output(String),
    Error(String),
    Quit,
    NoOp,
}

// ─── 내부 유틸 ────────────────────────────────────────────────────────────────

/// 간단한 공백/따옴표 토크나이저 (geulosh parser의 경량 버전).
fn tokenize_simple(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in s.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
            }
            ' ' | '\t' if !in_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

const REMOTE_HELP: &str = "\
GeulOS remote shell (--connect mode):
  help                          이 도움말
  exit | quit                   셸 종료
  mount text \"content\"          원격 서버에 Text 객체 마운트
  mount button \"label\"          원격 서버에 Button 객체 마운트
  invoke #N <method>            원격 객체 메서드 호출
  ls                            원격 서버의 객체 목록 (user:local 소유)

(M2: mount/invoke/ls 만 원격 지원. 나머지는 in-process 모드 사용.)";
