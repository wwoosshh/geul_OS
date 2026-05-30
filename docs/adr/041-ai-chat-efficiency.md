# ADR-041: AI 대화 효율 — inline result, fields filter, prompt caching, prompt hints

**상태:** 채택 (2026-05-30)
**관련:** ADR-031(chat session), ADR-038(async AI + JSONL log), `ai-bridge/src/system_prompt.md`

## 문제

VM 안 AI 세션이 객체 트리 명령을 효율적으로 호출하는지 처음으로 측정 가능해진 시점
(audit JSONL stderr mirror 추가 후), 단순한 1-파일 요약 요청 한 건에 **4 turn / 5 tool
call / 32.2초 / final 응답 2651자 / report_done summary 250자** 가 발생함을 확인했다.
audit 분석에서 네 가지 비효율 패턴이 드러났다.

### 패턴 1 — read-only invoke 후 강제 폴링

`Filesystem.read_external` 는 사실상 동기 read 동작인데 wire 프로토콜은 `event_id`만 반환
(fire-and-forget). AI가 결과를 보려면 별도 `get_object` 한 turn을 추가 호출해야 한다.

### 패턴 2 — get_object 응답의 over-fetching

`get_object`는 `acl(4개)`, `methods(2개 정의)`, `owner UUID`, `parent UUID`,
`props.root_path`, `granted_dirs`, `state.*` 전부를 반환한다. AI가 원한 것은 보통
`state` 한 필드인데 모든 메타가 history에 누적되어 다음 turn의 input token을 부풀린다.

### 패턴 3 — 무의미한 first list 시도

system_prompt가 "cwd 안 경로는 `list_objects_by_type("aios.std/File@1")` 우선"이라고
가르쳐 AI가 호스트 드라이브 경로(`C:\`/`D:\`)에서도 일단 그 list를 호출한다. 호스트
경로는 mount된 File 객체가 없으므로 결과가 항상 비어 turn 1개가 통째로 낭비된다.

### 패턴 4 — system_prompt + tools 정의 매 turn 중복 전송

system_prompt(~5KB) + tool 정의 6개가 매 turn input에 같이 들어간다. 캐시 마커 없이는
Anthropic API가 매번 full billing. 5+ turn 세션에서 누적 비용이 빠르게 커진다.

### 패턴 5 — report_done summary 과장

system_prompt에 "3-5 sentence Korean summary" 가이드가 있는데 실제 호출에서는 한
긴 문장 250자(예: "...주요 내용을 한국어로 구조화하여 요약 제공했습니다") + 부연
설명까지 들어가 output token 누수가 매 send마다 반복.

## 결정

다섯 개의 독립 fix를 ai-bridge layer + system_prompt 수준에서 적용한다. 모두
wire 프로토콜이나 server-host 변경 없이 클라이언트 측 레이어만 손댄다.

### A. `invoke_method` 응답에 read 결과 inline

`tools::dispatch_tool`에서 method가 `read` (File@1) 또는 `read_external`
(Filesystem@1)이면 invoke 직후 자동으로 짧은 polling(최대 200ms, 10ms 간격)으로
대상 객체의 state가 갱신될 때까지 대기 후 `state` 필드를 응답에 포함한다.

```rust
let auto_fetch = matches!(method, "read_external" | "read");
if auto_fetch {
    for _ in 0..20 {
        if let Ok(obj) = wire.get_object(target).await {
            let state = obj.get("state").cloned().unwrap_or(Value::Null);
            let ready = match method {
                "read_external" =>
                    state.get("last_read_path").and_then(|v| v.as_str())
                        == Some(path_arg.as_str())
                    && state.get("last_read_content").map(|v| !v.is_null()).unwrap_or(false),
                "read" => state.get("content").map(|v| !v.is_null()).unwrap_or(false),
                _ => true,
            };
            if ready {
                return Ok(DispatchResult::Output(json!({
                    "ok": true, "event_id": eid, "state": state,
                })));
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
```

**왜 폴링이 필요한가:** `wire.invoke` 완료 = server가 InvokeAck 받은 시점. 실제
handler의 SetState broadcast는 그 직후 별도 task로 발생 — race window 존재. 200ms
window가 사실상 모든 케이스를 잡는다. timeout 시 best-effort `state` + `stale: true`
플래그를 반환해 silent fail 방지.

### B. `get_object`에 `fields` 필터

`get_object`의 `input_schema`에 optional `fields: array<string>` 추가. top-level
필드만 받아 응답에서 그것만 남긴다.

```rust
let fields: Vec<String> = input.get("fields")
    .and_then(|v| v.as_array())
    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
    .unwrap_or_default();
match wire.get_object(id).await {
    Ok(obj) => {
        let filtered = if fields.is_empty() {
            obj
        } else if let Value::Object(map) = &obj {
            let mut out = serde_json::Map::new();
            for k in &fields { if let Some(v) = map.get(k) { out.insert(k.clone(), v.clone()); } }
            Value::Object(out)
        } else { obj };
        Ok(DispatchResult::Output(json!({ "object": filtered })))
    }
    Err(e) => Ok(DispatchResult::Output(json!({ "error": e.to_string() }))),
}
```

AI가 `fields: ["state"]`만 지정하면 acl/methods/owner/parent/props/children 등이
응답에서 제외되어 응답 크기 ~70% 절감. history에 누적되어 후속 turn input도 함께 절감.

### C. system_prompt에 호스트 드라이브 first-move 안내

system_prompt에 "Performance hints" 섹션 신설:

```markdown
- 호스트 드라이브 경로(`C:\`/`D:\` 등)는 mount된 `File@1` 객체가 거의 없음 —
  `list_objects_by_type("aios.std/File@1")` 빈 결과 예상되니 *건너뛰고*
  곧장 `list_objects_by_type("aios.builtin/Filesystem@1")` +
  `invoke_method(<fs_id>, "read_external", {path: ...})` 흐름으로.
  cwd 안 GeulOS 파일이면 `aios.std/File@1` list가 의미 있음.
```

AI가 즉시 Filesystem 단독 list로 직행 — 무의미 호출 1개 절감.

### D. Anthropic prompt caching

`ClaudeAdapter::complete`에서 system을 array-form content blocks로 변경하고
`cache_control: {type: "ephemeral"}` 마커를 system과 마지막 tool에 부착. 5-min TTL
캐시 — 첫 호출은 1.25x cache_creation 비용, 이후 호출은 cache_read 0.1x.

```rust
let system_blocks = json!([{
    "type": "text",
    "text": system,
    "cache_control": {"type": "ephemeral"},
}]);
if let Some(Value::Object(map)) = tools_json.last_mut() {
    map.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
}
```

검증을 위해 `parse_claude_response`에 `cache_read_input_tokens` /
`cache_creation_input_tokens`을 stderr로 dump해 시리얼 로그에서 hit 비율 확인 가능.

### E. report_done 길이 강제

`tools.rs`의 report_done description + system_prompt General rules 양쪽에서
"≤2 short sentences, ~30 words" 명시. AI가 이미 ai_text 본문에 요약을 제공했으므로
report_done은 *한 줄 액션 로그* 정도면 충분하다는 사용 의도를 설명.

## 측정

audit JSONL을 stderr로 mirror해 시리얼 로그에서 분석 가능하게 한 뒤, *동일한* 요청
("c:\AiOS\README.md 요약")으로 4회 측정.

| 단계 | Turn | Tool calls | Latency | Output (final) | Summary 길이 | Input (cache_read) |
|---|---|---|---|---|---|---|
| Baseline | 4 | 5 | 32.2s | 2651자 | 250자 | 0 (캐시 없음) |
| A+B 적용 | 3 | 3 (1차 race로 무효, B는 동작) | 30.5s | 2489자 | (long) | 0 |
| A polling 추가 | 3 | 3 | 28.7s | 2556자 | (long) | 0 |
| D prompt cache | 3 | 3 | **18.1s** (2번째 send) | 2780자 | (long) | **6530/6103** |
| C+E 적용 | 3 | 3 | 23.6s | 1869자 | **66자** | 6638 |

전체(baseline → C+E 후):
- Turn: 4 → **3** (-25%)
- Tool calls: 5 → **3** (-40%)
- Latency: 32.2s → **23.6s** (-27%)
- Final 응답: 2651자 → **1869자** (-30%)
- Summary: 250자 → **66자** (-74%)
- Input billing: 6KB × N turn → cache_read(0.1x) hit 매 turn (~85% 비용 절감)

## 결과

A polling은 race window가 실제 사용에서 거의 첫 iter에 잡혀 추가 latency 무시할
수준. D 캐시는 같은 wire를 재사용하는 후속 send에서 효과가 누적되어 길게 쓰는 세션일
수록 절감 폭이 커진다.

이 패턴은 read-only 동기 동작 (`File.read`, `Filesystem.read_external`) 한 쌍에만
적용했다. 다른 동기 메서드(`Folder.list`, `Folder.create_file/folder`, `File.save`,
`File.delete`, `File.rename` 등)도 같은 패턴(invoke 응답에 결과 inline)으로 확장 가능
— 후속 작업.

## 위험과 트레이드오프

**read 폴링 latency:** 200ms 최대 window를 추가. 실제로는 1-3 iter에 잡혀 5-30ms
선. 단 server SetState broadcast가 지연되면 timeout 시 stale state가 반환되어 AI가
이전 read 결과로 오해할 가능성. `stale: true` 플래그로 표시.

**캐시 마커 비용:** 첫 호출은 1.25x cache_creation 비용 (~+25% input 토큰).
2번째 호출부터 0.1x cache_read로 회수. 1-shot 세션에서는 손해 — 그러나 audit
로그상 대부분 세션이 3+ turn이므로 ROI 흑자.

**system_prompt 변경 = 캐시 무효화:** system_prompt를 수정할 때마다 첫 호출 다시
cache_creation. 운영 중 안내 텍스트 자주 수정하는 시점은 회피 (변경 전 5분 대기 후
일괄 적용 패턴 등).

**호스트 드라이브 안내가 너무 좁음:** "C:\/D:\ 경로면 list 생략" 규칙은 Windows
호스트 한정. macOS/Linux 호스트 마운트(예: `/mnt/host/...`)에서는 적용 안 됨 —
호스트 OS 일반화는 후속.

## 후속

- 쓰기/수정/삭제 메서드(`File.save`, `Folder.create_file`, `File.delete`,
  `File.rename`)에도 A 패턴 확장: invoke 응답에 작업 후 state 또는 ChildChange
  요약을 inline 포함해 AI가 별도 polling 없이 결과 인지.
- `get_object` `fields`에 dot-notation(`state.content` 단일 필드) 지원.
- `[claude-usage]` 로그를 audit JSONL에 정식 event로 통합 — 세션별 토큰 비용 추적.
