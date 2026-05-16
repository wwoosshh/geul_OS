# m1_smoke.gsh — M1 인수 시나리오 (geulosh로 검증)
# 본 스크립트는 M1의 모든 주요 기능을 사용한다:
# - mount (Container/Text/Button/Toggle)
# - ls / tree / get
# - invoke + ACL (owner 허용, 비-owner 거부)
# - query (type)
# - subscribe + drain
# - events 로그

# --- 1) 컨테이너 + 텍스트 ---
mount container
expect "#1"
expect "Container"

mount text "hello, GeulOS"
expect "#2"
expect "Text"

# 객체 상세 확인
get #2
expect "hello, GeulOS"

# --- 2) 트리/리스트 ---
ls
expect "#1"
expect "#2"

tree
expect "#1"

# --- 3) ACL: 다른 액터로 전환 후 권한 거부 ---
mount button "OK"
expect "#3"
expect "Button"

as ai
invoke #3 press
expect-error "권한"

# 본인이 만든 버튼은 OK
mount button "AI-OK"
expect "#4"
invoke #4 press
expect "Invoke event"

as user
# user가 만든 #3을 user가 누름 → 성공
invoke #3 press
expect "Invoke event"

# --- 4) Query ---
query type aios.std/Button@1
expect "#3"
expect "#4"

# --- 5) 구독 ---
subscribe #3 invoke
expect "@1"
expect "Subscribed"

invoke #3 press
expect "Invoke event"

drain @1
expect "Invoke"
expect "press"

# 두 번째 drain은 비어있어야 함
drain @1
expect "no events"

# 구독 해제
unsubscribe @1
expect "Unsubscribed"

# --- 6) Toggle ---
mount toggle on
expect "Toggle"

# --- 7) 전체 이벤트 로그 확인 ---
events 20
expect "Lifecycle"
expect "Invoke"

# --- 8) 종료 ---
exit
