//! GeulOS 와이어 클라이언트 (probe.py의 Rust 버전).
//!
//! 길이 접두사 JSON 프레임을 TCP로 송수신. `connect_as_ai`로 핸드셰이크 후
//! query/get/invoke/subscribe/drain/unsubscribe/mount 메서드 사용.

use geulos_core::{Object, ObjectId};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, GetMsg, GetResult, Hello, HelloAck, InvokeAck,
    InvokeMsg, MountAck, MountMsg, QueryMsg, QueryPredicate, QueryResult, Role, SubscribeAck,
    SubscribeMsg, UnsubscribeMsg,
};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

/// 와이어 클라이언트 에러.
#[derive(Debug, Error)]
pub enum WireError {
    /// IO 에러.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 직렬화/역직렬화 에러.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// 예상치 못한 응답 종류.
    #[error("unexpected response (got {got}, want {want})")]
    UnexpectedKind { want: String, got: String },
    /// 서버 측 에러 응답.
    #[error("server error: {kind} — {detail}")]
    ServerError { kind: String, detail: String },
    /// 요청이 deadline 안에 응답받지 못함 (KI-032).
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// 연결이 예기치 않게 종료됨.
    #[error("connection closed unexpectedly")]
    Closed,
}

pub type WireResult<T> = Result<T, WireError>;

/// GeulOS server-host에 TCP로 접속한 한 클라이언트.
pub struct WireClient {
    stream: TcpStream,
    actor_id: String,
    accum: Vec<u8>,
    request_timeout: Duration,
}

impl WireClient {
    /// `Role::Ai`로 핸드셰이크 + HelloAck 수신.
    pub async fn connect_as_ai(addr: &str) -> WireResult<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        let mut accum: Vec<u8> = Vec::new();
        let hello = Hello {
            version: "0.1".to_string(),
            role: Role::Ai,
            auth: Value::Object(Default::default()),
            client_id: "ai-bridge".to_string(),
        };
        let body = serde_json::to_vec(&hello)?;
        stream.write_all(&encode_frame(&body)).await?;

        let mut buf = vec![0u8; 4096];
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Err(WireError::Closed);
            }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(body) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let ack: HelloAck = serde_json::from_slice(&body)?;
                return Ok(Self {
                    stream,
                    actor_id: ack.actor_id,
                    accum,
                    request_timeout: Duration::from_secs(30),
                });
            }
        }
    }

    /// 발급된 actor id.
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// 요청 deadline 변경 (기본 30s). 테스트·튜닝용.
    pub fn with_request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }

    /// 한 프레임 송신 + 한 프레임 수신.
    /// 송신 msg의 request_id를 기억해, 응답의 request_id가 일치할 때까지 broadcast
    /// 프레임은 skip. subscribe 후 SetState/Invoke event broadcast가 도착해 후속 RPC
    /// 응답에 끼는 race(예: ShellRunner SetState 다발 발생 시) 회피.
    async fn request(&mut self, msg: &Value) -> WireResult<Value> {
        let expected_rid = msg.get("request_id").and_then(|v| v.as_str()).map(String::from);
        let body = serde_json::to_vec(msg)?;
        self.stream.write_all(&encode_frame(&body)).await?;
        let timeout = self.request_timeout;
        // expected_rid 있으면 일치할 때까지 frame skip; 없으면 첫 frame 반환.
        // 전체 루프를 deadline으로 감싸 서버 무응답/broadcast 폭주 시 hang 방지 (KI-032).
        let fut = async {
            loop {
                let frame = self.read_frame_json().await?;
                if let Some(rid) = &expected_rid {
                    let got = frame.get("request_id").and_then(|v| v.as_str());
                    if got == Some(rid.as_str()) {
                        return Ok(frame);
                    }
                    // request_id 다르거나 없음 → broadcast event. skip + 다음 frame.
                    continue;
                }
                return Ok(frame);
            }
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => Err(WireError::Timeout(timeout)),
        }
    }

    /// 한 프레임 수신 (대기).
    async fn read_frame_json(&mut self) -> WireResult<Value> {
        let mut buf = vec![0u8; 4096];
        loop {
            let mut slice = self.accum.as_slice();
            if let Ok(body) = decode_frame(&mut slice) {
                let consumed = self.accum.len() - slice.len();
                self.accum.drain(..consumed);
                return Ok(serde_json::from_slice(&body)?);
            }
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                return Err(WireError::Closed);
            }
            self.accum.extend_from_slice(&buf[..n]);
        }
    }

    /// 객체 mount. 서버가 부여한 (또는 클라이언트가 만든) root ObjectId 반환.
    pub async fn mount(&mut self, obj: Object) -> WireResult<ObjectId> {
        let id = obj.id;
        let msg = MountMsg { root_object_id: id.to_string(), tree: serde_json::to_value(&obj)? };
        let resp = self.request(&serde_json::to_value(&msg)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("MountAck") => {
                let _ack: MountAck = serde_json::from_value(resp)?;
                Ok(id)
            }
            Some("MountReject") => Err(WireError::ServerError {
                kind: "mount_reject".to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
            other => Err(WireError::UnexpectedKind {
                want: "MountAck".to_string(),
                got: other.unwrap_or("?").to_string(),
            }),
        }
    }

    /// Query by type. 객체 ID 문자열 목록 반환.
    pub async fn query_by_type(&mut self, type_uri: &str) -> WireResult<Vec<String>> {
        let q = QueryMsg {
            request_id: format!("q-{}", Uuid::new_v4()),
            query: QueryPredicate::ByType { type_uri: type_uri.to_string() },
        };
        let resp = self.request(&serde_json::to_value(&q)?).await?;
        let r: QueryResult = serde_json::from_value(resp)?;
        Ok(r.objects)
    }

    /// Get object — JSON value 반환.
    pub async fn get_object(&mut self, object_id: &str) -> WireResult<Value> {
        let g =
            GetMsg { request_id: format!("g-{}", Uuid::new_v4()), target: object_id.to_string() };
        let resp = self.request(&serde_json::to_value(&g)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("GetResult") => {
                let r: GetResult = serde_json::from_value(resp)?;
                Ok(r.object)
            }
            _ => Err(WireError::ServerError {
                kind: resp
                    .get("error_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
        }
    }

    /// Invoke. event_id 문자열 반환.
    pub async fn invoke(&mut self, target: &str, method: &str, args: Value) -> WireResult<String> {
        let i = InvokeMsg {
            request_id: format!("i-{}", Uuid::new_v4()),
            target: target.to_string(),
            method: method.to_string(),
            args,
        };
        let resp = self.request(&serde_json::to_value(&i)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("InvokeAck") => {
                let a: InvokeAck = serde_json::from_value(resp)?;
                Ok(a.event_id)
            }
            _ => Err(WireError::ServerError {
                kind: resp
                    .get("error_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
        }
    }

    /// Subscribe. subscription_id 반환.
    pub async fn subscribe(
        &mut self,
        target: &str,
        kinds: &[EventKindFilterWire],
    ) -> WireResult<String> {
        let sid = format!("sub-{}", Uuid::new_v4());
        let s = SubscribeMsg {
            subscription_id: sid.clone(),
            target: target.to_string(),
            kinds: kinds.to_vec(),
            include_initial: false,
        };
        let resp = self.request(&serde_json::to_value(&s)?).await?;
        let _ack: SubscribeAck = serde_json::from_value(resp)?;
        Ok(sid)
    }

    /// Drain — 큐에 쌓인 이벤트가 있다면 *지금* 도착한 것까지 모두 수집.
    /// 없으면 빈 vec. (서버는 이벤트를 push해두므로, 이 호출은 *수신 버퍼 비우기*.)
    /// 짧은 타임아웃(~100ms)으로 polling.
    pub async fn drain(&mut self, _subscription_id: &str) -> WireResult<Vec<Value>> {
        let mut buf = vec![0u8; 4096];
        let mut events = Vec::new();
        let timeout = std::time::Duration::from_millis(150);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // 이미 accum에 있는 메시지 먼저 추출
            loop {
                let mut slice = self.accum.as_slice();
                match decode_frame(&mut slice) {
                    Ok(body) => {
                        let consumed = self.accum.len() - slice.len();
                        self.accum.drain(..consumed);
                        let v: Value = serde_json::from_slice(&body)?;
                        if v.get("kind").and_then(|k| k.as_str()) == Some("Event") {
                            events.push(v);
                        }
                    }
                    Err(_) => break,
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline - now;
            let r = tokio::time::timeout(remaining, self.stream.read(&mut buf)).await;
            match r {
                Ok(Ok(0)) => return Err(WireError::Closed),
                Ok(Ok(n)) => self.accum.extend_from_slice(&buf[..n]),
                Ok(Err(e)) => return Err(WireError::Io(e)),
                Err(_) => break, // timeout
            }
        }
        Ok(events)
    }

    /// Unsubscribe (응답 없음).
    pub async fn unsubscribe(&mut self, subscription_id: &str) -> WireResult<()> {
        let u = UnsubscribeMsg { subscription_id: subscription_id.to_string() };
        let body = serde_json::to_vec(&u)?;
        self.stream.write_all(&encode_frame(&body)).await?;
        Ok(())
    }
}
