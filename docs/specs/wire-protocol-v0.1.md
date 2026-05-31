> **Status:** adopted (2026-05-31)
> **Note:** M2에서 v0.1 동결 후 모든 후속 마일스톤(M3~M13 + VM 컴포지터 + Host Bridge)에서 그대로 사용 중. JSON over TCP 프레이밍·Hello 핸드셰이크·invoke/subscribe 구조 유지.

# GeulOS 와이어 프로토콜 스펙 v0.1

- **상태:** Draft v0.1
- **일자:** 2026-05-17
- **저자:** wwoosshh, with Claude
- **재결정 시점:** M2 완료 시 v1.0 동결 여부 검토

## 0. 위치

이 스펙은 외부 프로세스(AI 클라이언트, 앱, 컴포지터)와 GeulOS 객체 서버 사이의 *유일한* 메시지 포맷을 정의한다. 본 스펙의 구현은 M2 마일스톤의 산출물.

## 1. 전송 (Transport)

| 클라이언트 | 엔드포인트 | 인증 |
|---|---|---|
| AI (VM 내부) | `/run/aios/ai.sock` (Unix) | 세션 토큰 |
| AI (VM 외부) | TCP `:<port>` (M6에서 결정) | mTLS 또는 세션 토큰 |
| 앱 | `/run/aios/app.sock` (Unix) | 매니페스트 |
| 컴포지터 | (커널 내부 IPC, 단일 신뢰 채널) | 권한 매니저가 직접 발급 |

연결은 양방향 스트림. 프레이밍은 *4바이트 빅엔디언 길이 접두사 + 본문 JSON*. 본문은 UTF-8 인코딩.

향후 v1.0에서 바이너리 포맷(MessagePack 또는 CBOR) 전환 검토 — 지금은 디버깅 용이성을 우선.

## 2. 핸드셰이크

연결 직후 클라이언트가 가장 먼저 `Hello`를 보낸다. 서버는 `HelloAck` 또는 `HelloReject`로 응답.

### Hello (client → server)

```json
{
  "kind": "Hello",
  "version": "0.1",
  "role": "ai" | "app" | "compositor",
  "auth": { "token": "..." } | { "manifest": { ... } },
  "client_id": "<sender 자유 식별자>"
}
```

### HelloAck (server → client)

```json
{
  "kind": "HelloAck",
  "session_id": "<UUID>",
  "actor_id": "<UUID>",
  "server_version": "0.1",
  "capabilities": ["mount", "invoke", "subscribe", "query", "glscript"]
}
```

### HelloReject (server → client)

```json
{
  "kind": "HelloReject",
  "reason": "version_mismatch" | "auth_failed" | "role_unknown" | "...",
  "detail": "사람이 읽을 수 있는 설명"
}
```

## 3. 메시지 종류 (7개)

### 3.1 Mount (app → server)

앱이 자기 객체 서브트리를 객체 서버에 게시.

```json
{
  "kind": "Mount",
  "root_object_id": "<ObjectId, 클라이언트 발급 임시 ID 또는 서버 위임>",
  "tree": {
    "id": "...",
    "type_uri": "aios.std/Window@1",
    "props": { "title": "메모장" },
    "state": {},
    "methods": [...],
    "children": [...]
  }
}
```

응답: `MountAck { server_assigned_ids: {...} }` 또는 `MountReject`.

### 3.2 Invoke (client → server)

객체의 메서드 호출.

```json
{
  "kind": "Invoke",
  "request_id": "<ULID, 클라이언트가 발급, 응답 매칭용>",
  "target": "<ObjectId>",
  "method": "press",
  "args": { ... }
}
```

응답: `InvokeAck { request_id, event_id, result }` 또는 `InvokeError { request_id, kind: "permission" | "no_such_object" | ..., detail }`.

### 3.3 Subscribe (client → server)

객체/서브트리 변화 구독.

```json
{
  "kind": "Subscribe",
  "subscription_id": "<클라이언트 발급 ID>",
  "target": "<ObjectId 또는 패턴>",
  "kinds": ["StateSet", "Lifecycle"],
  "include_initial": true
}
```

응답: `SubscribeAck { subscription_id }`.

### 3.4 Unsubscribe (client → server)

```json
{
  "kind": "Unsubscribe",
  "subscription_id": "<발급 시 받은 ID>"
}
```

### 3.5 Query (client → server)

상태 단발 조회.

```json
{
  "kind": "Query",
  "request_id": "<ULID>",
  "query": {
    "type": "type=Memo",
    "depth": 2
  }
}
```

응답: `QueryResult { request_id, objects: [...] }`.

### 3.6 Event (server → client)

객체 상태 변화 통보 (Subscribe된 클라이언트에게).

```json
{
  "kind": "Event",
  "subscription_id": "<발급 시 받은 ID>",
  "event": {
    "id": "<EventId, 단조 증가>",
    "actor": "<ActorId>",
    "target": "<ObjectId>",
    "kind": "Invoke" | "StateSet" | "Lifecycle",
    "payload": { ... },
    "causation": "<EventId 또는 null>"
  }
}
```

### 3.7 Glscript (AI → server)

AI가 보낸 글 코드 한 덩어리 실행.

```json
{
  "kind": "Glscript",
  "request_id": "<ULID>",
  "source": "<글 소스 코드 문자열>",
  "budget": {
    "max_opcodes": 100000,
    "max_memory_bytes": 16777216,
    "max_wall_ms": 5000
  }
}
```

응답: `GlscriptResult { request_id, exit_code, events: [...], stdout, stderr }` 또는 `GlscriptError`.

## 4. 액터(ActorId) 모델

| Actor 종류 | ActorId 형식 | 발급 시점 |
|---|---|---|
| 사용자 (콘솔 사용자) | `user:local` | 부팅 시 고정 |
| AI 세션 | `ai:<UUID>` | Hello 시 |
| 앱 | `app:<manifest.id>:<instance UUID>` | Mount 시 |
| 컴포지터 | `system:compositor` | 부팅 시 고정 |

## 5. 권한 검사

모든 `Invoke`와 `Mount` 안의 메서드 정의는 권한 매니저의 ACL을 통과해야 한다. 거부 시 `InvokeError { kind: "permission" }`.

## 6. 호환성

v0.1은 *깨질 수 있는 버전*. M2 완료 시 v1.0으로 동결 검토. 동결 후 메이저 버전은 의미 변경 시에만 증가.

## 7. 미해결 항목 (M2 작업으로 이관)

- 바이너리 포맷 (MessagePack vs CBOR)
- 스트리밍 응답 (긴 `Glscript`의 중간 stdout 흐름)
- 압축
- 멀티플렉싱 (한 연결에 여러 세션)

## 8. 참고

- 설계 문서: `docs/specs/2026-05-17-geulos-design.md` §5.3
