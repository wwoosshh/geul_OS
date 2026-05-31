> **Status:** adopted (2026-05-31)
> **Note:** ADR-041 효율 패턴 mutation 확장으로 채택 — 2초 polling + 명시적 pending 상태(Option C) 정착, README M11.2 효율 항목 매핑.

# AI mutation 효율 spec — save / create / delete / rename

**상태:** 초안 (사용자 검토 대기)
**선행:** ADR-041 (AI 대화 효율 — read 계열)
**범위:** `File.save`, `Folder.create_file`, `Folder.create_folder`, `File.delete`,
`File.rename`, `Folder.delete`, `Folder.rename`, `Filesystem.write_external`

## 문제

ADR-041에서 *read-only* invoke(`File.read`, `Filesystem.read_external`)는 결과를
응답 `state`에 inline 반환하도록 만들었다. mutation 계열에는 같은 패턴이 그대로
적용되지 않는다 — 가장 큰 차이는 **사용자 Dialog 승인 대기**다.

mutation invoke의 현재 흐름:

1. AI → `invoke_method(target, "save", {content: ...})` → desktop-shell이 `Dialog@1`
   mount + 사용자에게 [허용]/[거부] 제시.
2. wire는 *invoke ack* (event_id)만 즉시 반환. dialog 응답은 분리된 비동기 이벤트.
3. 사용자가 응답할 때까지 1-30초(평균 ~3-5초) 사이 idle. AI 측에서는 결과 미상.
4. 사용자 [허용] → desktop-shell이 실제 fs::write/create/remove 실행 → SetState
   broadcast(state.dirty=false, state.size, ChildChange 등).
5. 사용자 [거부] → desktop-shell이 ack 후 dialog destroyed, **상태 변경 없음**.

ADR-041의 200ms polling 패턴을 그대로 쓸 수 없는 이유:

- Dialog 대기 시간이 polling window보다 훨씬 큼.
- AI가 30초 polling을 안에서 돌면 다른 사용자 입력도 못 받음 (wire monopolize).
- 사용자가 응답 안 하는 경우(자리 비움) 영원히 hang 가능.
- 거부 신호를 어떻게 인지할지 별도 결정 필요.

## 핵심 의사결정 — Dialog 승인 대기 타이밍

**채택:** Option C — 짧은 window polling + 명시적 pending 상태.

### 검토한 대안

**Option A: 30초 같은 긴 polling으로 사용자 응답까지 대기**
- 장점: AI가 한 turn 안에 mutation 완결 인지 — turn 절감.
- 단점:
  - wire monopolize — 그 30초 동안 AI가 다른 동작 불가.
  - 사용자 부재 시 timeout 토큰 비용 큼 (긴 LLM tool_use loop).
  - Dialog는 본질적으로 사용자 페이스 — 강제 동기화는 UX 충돌.
- 기각.

**Option B: 즉시 event_id만 반환, AI에게 명시 polling 가이드 (현재)**
- 장점: 단순. wire 점유 없음.
- 단점:
  - AI가 결과 인지하려면 별도 turn으로 `subscribe + drain` 또는 `get_object` 폴링.
  - 거부/timeout 신호 모호 — AI가 "혹시 끝났나" repeated polling 유발.
  - turn 폭주 위험 (사용자 미응답 + AI는 계속 확인).
- 기각.

**Option C: 짧은 window(2초) polling 후 명시 pending 상태 반환 ✅**
- 흐름:
  1. invoke 직후 최대 2초간 100ms 간격으로 dialog 상태 폴링.
  2. 2초 안에 사용자가 응답하면 결과/거부 신호 inline 반환 (read 패턴과 동일).
  3. 2초 timeout 시 응답: `{ok: true, event_id, pending_dialog: <dialog_id>, status: "awaiting_user"}`.
  4. AI는 pending_dialog id를 받아 *원할 때* `subscribe(<dialog_id>, ["Lifecycle"])` 또는
     사용자에게 "대기 중" 안내 후 다음 user prompt에서 결과 polling.
- 장점:
  - 사용자가 즉시 응답한 경우 turn 1개 절감 (read와 동일 ergonomics).
  - 부재 시 wire monopolize 없음.
  - pending 신호가 명확 — AI가 "내가 모르는 게 뭔지" 정확히 알 수 있음.
- 트레이드오프:
  - dialog 응답이 정확히 2-3초 사이에 떨어지면 race가 있을 수 있음 — `subscribe`로
    fallback 안내가 spec에 명시되어야 함.
  - polling 100ms × 20회는 read의 10ms × 20회보다 latency 추가지만 dialog는 본질적으로
    초 단위 동작이라 허용 가능.

### 2초 window 근거

`docs/manual-tests`의 dialog UX 관측: 사용자가 *명백한* 동작(자기가 직접 시킨 파일
저장)에 대해 평균 ~1.5초 안에 응답. *예상치 못한* mutation은 3-10초까지 갈 수 있음 —
이건 의도된 신중함이므로 강제 가속 X. 2초는 "즉시" 응답을 잡고 그 외는 pending으로
넘기는 합리적 컷.

5초로 늘리면 캐치율 +10-15%, 그러나 wire 점유 시간도 +150%. ROI 낮음.

## 명시적 결정 사항

### D1: polling 대상

dialog 객체의 lifecycle/state를 폴링하는 게 아니라 *target object* state를 폴링한다
(read 패턴 일관성). 이유:

- `Dialog@1.respond(action)`이 호출되면 desktop-shell이 dialog destroy + 실제 mutation
  적용 + target state SetState 까지 한 sync block에서 처리.
- target.state.dirty/size/destroyed 등이 변하면 ack 받은 것.
- dialog id를 ai-bridge가 알기 위해 wire response에 새 필드 필요 (현재 invoke는
  event_id만) — 변경 범위 큼. target state만 보면 spec 단순화.

다만 거부 신호 식별을 위해 보조로 dialog 객체 list가 비었는지(destroyed) 같이 본다 —
2초 안에 dialog가 destroyed면서 target state 미변경이면 거부로 판정.

### D2: 메서드별 ready 조건

target state 어떤 필드가 갱신되면 "성공"으로 인지할지 메서드별 정의:

| Method | Ready 신호 |
|---|---|
| `File.save` | `state.dirty == false` (이전 `true`였다가 변경) |
| `Folder.create_file` | parent의 `state.child_count` 증가 또는 `children` 길이 증가 |
| `Folder.create_folder` | 동일 |
| `File.delete` | target의 `state.destroyed == true` |
| `Folder.delete` | 동일 |
| `File.rename` | `state.path` 또는 `props.name` 변경 |
| `Folder.rename` | 동일 |
| `Filesystem.write_external` | `state.last_write_path == args.path` (신규 필드 도입 필요) |

`create_*`은 새 child id를 응답에 포함하면 후속 호출에서 유용. parent의 `children`
배열 diff(이전 길이 → 현재 길이)로 식별 가능.

### D3: 거부 신호

ai-bridge 응답에 `status` 필드 추가:

- `"completed"`: 2초 안에 사용자 [허용] + state 갱신 확인.
- `"rejected"`: 2초 안에 dialog가 destroyed되었으나 target state 미변경 (거부 추정).
- `"awaiting_user"`: 2초 timeout — 사용자 아직 미응답. `pending_dialog` id 포함.

AI는 status를 보고 그에 맞게 사용자에게 안내. system_prompt에 status 의미 명시.

### D4: 응답 페이로드 (사용자 선택 — 보조 결정)

ADR-041 read 패턴과 일관성:

```json
{
  "ok": true,
  "event_id": "ev:NNN",
  "status": "completed" | "rejected" | "awaiting_user",
  "state": { ... target state (변경된 필드 포함) },
  "new_child": { "id": "...", "name": "...", "path": "..." }  // create_* 한정
}
```

`fields` 옵션 추가 가능 (B 패턴 — get_object와 동일) — V2.

## Grant 자동화 — 부차 결정

desktop-shell은 이미 *dir 단위 grant* 모델을 갖고 있다 (system_prompt 언급).
첫 mutation 후 그 directory 안 후속 write/create/rename은 dialog 없이 통과.
**현재 spec에서는 별도 변경 없음** — 다음만 spec에 추가 명시:

- pending_dialog 응답에 그 시점 `granted_dirs` snapshot 함께 포함 (Filesystem singleton의
  state 일부). AI가 "어떤 폴더가 이미 grant 됐는지" 명시적으로 안다 → 같은 dir에 추가
  mutation 시 polling window 안에서 sure completed 받을 확률 ↑.
- 별도 도구 추가는 없음. 기존 `list_objects_by_type("aios.builtin/Filesystem@1") +
  get_object(fields=["state"])`로 동일 정보 획득 가능 — 단지 mutation 응답에 동봉해
  추가 turn 절감.

## 구현 범위

ai-bridge `tools::dispatch_tool`의 `"invoke_method"` 분기에 mutation 메서드별
ready 조건 + status 분기 추가. wire 프로토콜 / server-host / desktop-shell handler
무변경 (Filesystem.write_external의 `last_write_path` 필드 SetState만 desktop-shell
external_methods.rs에 한 줄 추가).

총 변경 예상: ai-bridge 1 파일(~80 line), desktop-shell 1 파일(~10 line),
system_prompt.md (~20 line — Performance hints 섹션 확장).

## 측정 계획

ADR-041과 동일하게 audit JSONL stderr mirror로 측정.

baseline 시나리오 (현재):
- "테스트 폴더 만들고 거기에 hello.txt 저장" → 예상 turn count ?

A+B+C+D 적용 후 동일 시나리오 측정. 목표:
- 사용자가 즉시 [허용]하는 시나리오: turn 절감 1-2개.
- 사용자가 늦게 응답하는 시나리오: turn 동일하지만 AI가 "대기 중" 안내 가능 (UX).
- 사용자가 거부하는 시나리오: AI가 *명시적* rejected 신호 받음 (현재는 silent fail
  추정 후 turn 폭주).

## 트레이드오프

**polling latency:** 모든 mutation invoke에 최대 2초 추가. 즉시 [허용] 시나리오에서
실제 latency는 ~100-500ms (1-5 iter). 실측 필요.

**dialog destroyed != rejected 가능성:** 사용자가 dialog 닫기 X 버튼 등으로 닫는 경우
도 destroyed로 보일 수 있음. desktop-shell의 close vs respond(reject) 구분 확인 필요
(추정 — `Dialog.respond("reject")`만 dialog destroy + 별도 신호 없으면 두 경우 같음).
보수적으로 두 케이스 모두 `rejected`로 묶기.

**create_* new_child id 식별 race:** parent.children 배열 diff로 새 child 식별 시 동시
다른 source(사용자가 직접 Explorer에서 만든 file)가 끼면 ambiguous. 단순화: invoke
직전/직후 children id 집합 diff로 id 1개만 추가됐으면 그것을 new_child, 여러 개면
응답에 모두 포함하고 "ambiguous: true" 플래그.

**Filesystem.write_external의 last_write_path 필드 추가:** desktop-shell external_methods
변경 필요. server state schema 확장이라 다른 client(host compositor)도 영향 — 단,
optional 필드라 호환성 안전.

## 적용 후 다음 단계

이 spec이 처리되면 mutation 계열도 read와 동등한 ergonomics. 그 다음 후보:

- `Folder.list`도 inline 결과 반환 (read 패턴) — 현재 list는 fire-and-forget,
  AI가 list_objects_by_type / get_object로 재조회 필요.
- `subscribe + drain` 의존 시나리오 (ConsoleWindow streaming 등)는 별도 패턴 — 본
  spec 범위 밖.
- `[claude-usage]` event를 audit JSONL 정식 event로 통합 — 토큰 비용 세션별 추적.
