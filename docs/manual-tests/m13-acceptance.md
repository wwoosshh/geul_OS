# M13 ConsoleWindow@1 — 수동 acceptance 시나리오

**Spec:** `docs/specs/2026-05-28-geulos-m13-console-window.md`
**Plan:** `docs/plans/2026-05-28-geulos-m13-console-window.md`

각 시나리오는 *전제* (사전 상태) + *행동* + *예상 결과*로 구성. 통과 시 ✅ 표시.

## 사전 준비

1. `Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue`
2. `cargo build --bin geulos`
3. `D:/GeulOS/target/debug/geulos.exe` (background)
4. ANTHROPIC_API_KEY 환경 변수 또는 `~/.geulos/api_key` 준비

## 시나리오 1: 단순 long-running echo loop

**전제:** launcher 띄움. desktop-shell + compositor 동작.

**행동:** compositor CLI에서 (또는 controller 외부 client에서) ShellRunner singleton에 직접 invoke:

```
run_streamed cmd=node args=["-e","setInterval(()=>console.log('tick',Date.now()),500)"] cwd=D:/GeulOS
```

**예상:**
- ConsoleWindow가 desktop에 floating panel로 표시 (cascade 위치)
- titlebar: `"node -e setInterval... — GeulOS"`, status dot 초록
- 본문에 0.5초마다 `tick 1716...` line 추가
- 500 line 도달 후 가장 오래된 line이 pop_front
- line_count는 계속 증가
- titlebar `[showing 500 of 1234]` 표시 (선택)

✅ / ❌

## 시나리오 2: stderr 색상 구분

**행동:**
```
run_streamed cmd=node args=["-e","console.log('stdout');console.error('stderr');"] cwd=D:/GeulOS
```

**예상:**
- 본문에 `stdout` (기본 색) + `[stderr] stderr` (약간 다른 색) 표시
- 0.1초 후 exit code 0 → status 회색

✅ / ❌

## 시나리오 3: 사용자 X 닫기 → cascade kill

**전제:** 시나리오 1의 ConsoleWindow 띄움 (node interval 돌고 있음).

**행동:** ConsoleWindow titlebar X 버튼 클릭.

**예상:**
- 1초 안에 status 빨강 (terminated)
- titlebar dot 회색/빨강
- `Get-Process node` 0 (cascade kill 확인 — npm spawn 시 손주 포함)
- ConsoleWindow는 desktop에 *그대로 남음* (history 확인 가능 — UI 닫기 별 동작은 v2)

✅ / ❌

## 시나리오 4: vite dev server + URL 안내

**전제:** `D:/GeulOS/tmp-react-app`에 `npm install` 완료된 react 프로젝트 (또는 `cargo run --example auto_react_dev_server`로 자동 생성).

**행동:** AI에게 prompt:
> "tmp-react-app에서 vite dev server를 띄우고 Local URL을 알려줘."

**예상:**
- AI가 `run_streamed cmd=npm args=["run","dev"] cwd=D:/GeulOS/tmp-react-app`
- Dialog 표시 → 사용자 [허용]
- ConsoleWindow mount, 본문에 vite 시작 로그 stream
- AI가 `Local:   http://localhost:5173/` 발견 → 사용자에게 "http://localhost:5173에서 확인하세요" 안내
- 사용자가 브라우저에서 접속 → react 앱 표시

✅ / ❌

## 시나리오 5: AI terminate → Dialog 동의 후 cascade kill

**전제:** 시나리오 4의 dev server 동작 중.

**행동:** AI에게 prompt:
> "이제 dev server 종료해줘."

**예상:**
- AI가 `invoke_method(<cw_id>, "terminate", {})`
- 사용자 Dialog "AI가 'npm run dev' 프로세스 종료를 요청합니다. 허용?"
- 사용자 [허용]
- ConsoleWindow status 빨강 (terminated)
- `Get-Process node` 0 (vite + esbuild 등 손주 포함)

✅ / ❌

## 시나리오 6: AI subscribe race 폴백 (KI-026)

**행동:** AI가 매우 짧은 명령으로 run_streamed:
```
run_streamed cmd=node args=["-e","console.log('hi');process.exit(0)"] cwd=D:/GeulOS
```

**예상:**
- AI subscribe 시점이 이미 exit 후라 drain empty
- AI가 *get_object 폴백*으로 state.lines=["hi"], status="exited", exit_code=0 확인
- AI가 사용자에게 정상 보고

✅ / ❌

## 종합 통과 기준

- 6 시나리오 모두 ✅
- `Get-Process node` (시연 후 cleanup): 0
- `cargo test --workspace` 모두 PASS
- `cargo clippy --workspace --all-targets -- -D warnings` clean
