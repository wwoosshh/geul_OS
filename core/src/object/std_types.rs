//! 표준 객체 팩토리 함수.
//!
//! 모든 GeulOS 앱이 기본적으로 사용하게 되는 표준 객체 타입:
//! `Container`, `Text`, `Button`, `Toggle` (M3),
//! `Memo`, `TextArea`, `MemoList` (M7 — 메모장 도그푸딩),
//! `Desktop`, `FileTree`, `Canvas`, `Folder`, `File` (M7 — 데스크톱 셸),
//! `Cli` (M7 T7.5 — 하단 CLI 패널, 셸의 일급 구성요소),
//! `Window`, `Explorer` (M8 — 멀티-윈도우 + 우측 탐색기).

use serde_json::json;

use super::identity::{ActorId, ObjectId, TypeUri};
use super::method::{ArgSpec, MethodSig};
use super::Object;

/// 레이아웃 컨테이너. 자식 객체를 담는 용도.
pub fn container(owner: ActorId) -> Object {
    Object::new(TypeUri::parse("aios.std/Container@1").expect("유효한 TypeUri"), owner)
}

/// 텍스트 표시 객체.
pub fn text(owner: ActorId, content: &str) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Text@1").expect("유효한 TypeUri"), owner);
    obj.set_state("content", json!(content));
    obj
}

/// 누를 수 있는 버튼.
pub fn button(owner: ActorId, label: &str) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Button@1").expect("유효한 TypeUri"), owner);
    obj.set_state("label", json!(label));
    obj.methods.push(MethodSig::new("press"));
    obj
}

/// 켜고 끄는 토글.
pub fn toggle(owner: ActorId, initial: bool) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Toggle@1").expect("유효한 TypeUri"), owner);
    obj.set_state("on", json!(initial));
    obj.methods.push(MethodSig::new("toggle"));
    obj.methods.push(MethodSig::new("set"));
    obj
}

// ───────────────────────── M7: 메모장 타입 ─────────────────────────

/// 메모 한 건.
///
/// state:
/// - `title: String` — 메모 제목
/// - `body: String` — 메모 본문 (UTF-8, byte index 기반 편집)
/// - `created_at: i64` — Unix ms 생성 시각
/// - `updated_at: i64` — Unix ms 마지막 수정 시각
/// - `tags: [String]` — 사용자 또는 AI가 부여한 태그
///
/// 메서드:
/// - `insert_text(at: usize, text: String)` — body의 byte index `at`에 `text` 삽입
/// - `delete_range(from: usize, to: usize)` — body의 [from, to) byte 범위 삭제
/// - `set_title(title: String)` — 제목 변경
/// - `set_tags(tags: [String])` — 태그 교체 (병합 아님)
/// - `save()` — 영속 저장 (notepad-app이 fs로 flush)
pub fn memo(owner: ActorId, title: &str, created_at_ms: i64) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Memo@1").expect("유효한 TypeUri"), owner);
    obj.set_state("title", json!(title));
    obj.set_state("body", json!(""));
    obj.set_state("created_at", json!(created_at_ms));
    obj.set_state("updated_at", json!(created_at_ms));
    obj.set_state("tags", json!([] as [&str; 0]));

    obj.methods.push(
        MethodSig::new("insert_text")
            .with_arg(ArgSpec::new("at", "usize"))
            .with_arg(ArgSpec::new("text", "string")),
    );
    obj.methods.push(
        MethodSig::new("delete_range")
            .with_arg(ArgSpec::new("from", "usize"))
            .with_arg(ArgSpec::new("to", "usize")),
    );
    obj.methods.push(MethodSig::new("set_title").with_arg(ArgSpec::new("title", "string")));
    obj.methods.push(MethodSig::new("set_tags").with_arg(ArgSpec::new("tags", "[string]")));
    obj.methods.push(MethodSig::new("save"));
    obj
}

/// 편집 가능한 텍스트 위젯. *컴포지터가 직접 다루며 와이어 메서드는 노출하지 않음*.
///
/// props:
/// - `bound_memo: ObjectId` — 이 TextArea가 보여주는 Memo 객체
///
/// state (compositor가 갱신):
/// - `cursor_pos: usize` — body 안 커서 위치 (byte index)
/// - `selection: Option<[usize, usize]>` — 선택 영역
/// - `focused: bool` — 키보드 입력 수신 여부
///
/// 사용자/AI가 body를 *직접* 변경하지 않고 bound_memo의 메서드를 호출 — 그래야 단일
/// 라이터 이벤트 루프와 영속성 모델이 흐트러지지 않는다.
pub fn text_area(owner: ActorId, bound_memo: ObjectId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/TextArea@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("bound_memo", json!(bound_memo));
    obj.set_state("cursor_pos", json!(0));
    obj.set_state("selection", json!(null));
    obj.set_state("focused", json!(false));
    obj
}

/// 메모 목록 컨테이너. notepad-app이 루트로 mount, 자식이 Memo 객체들.
///
/// state:
/// - `active_memo: Option<ObjectId>` — 현재 편집 중인 메모
///
/// 메서드:
/// - `create_memo(title: String)` — 새 Memo 생성 + 자식으로 추가
/// - `delete_memo(id: ObjectId)` — Memo destroy + fs 파일 제거
/// - `set_active(id: ObjectId)` — TextArea의 bound_memo를 갱신
pub fn memo_list(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/MemoList@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_memo", json!(null));

    obj.methods.push(MethodSig::new("create_memo").with_arg(ArgSpec::new("title", "string")));
    obj.methods.push(MethodSig::new("delete_memo").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("set_active").with_arg(ArgSpec::new("id", "ObjectId")));
    obj
}

// ───────────────────────── M7: 데스크톱 셸 타입 ─────────────────────────

/// 데스크톱 루트 셸. 컴포지터가 좌/우 분할로 그림.
///
/// 자식: [FileTree, Canvas] 순서.
pub fn desktop(owner: ActorId) -> Object {
    Object::new(TypeUri::parse("aios.builtin/Desktop@1").expect("유효한 TypeUri"), owner)
}

/// 좌측 파일 트리 패널.
///
/// props:
/// - `root_path: String` — 워크스페이스 절대 경로 (표시·디버그용)
///
/// state:
/// - `expanded: [ObjectId]` — 펼쳐진 폴더 ID 목록
/// - `selected: Option<ObjectId>` — 현재 선택된 노드 (FileTree 안에서 강조)
///
/// 메서드: `expand(id)`, `collapse(id)`, `select(id)`, `refresh()` — 재스캔
pub fn file_tree(owner: ActorId, root_path: &str) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/FileTree@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("root_path", json!(root_path));
    obj.set_state("expanded", json!([] as [&str; 0]));
    obj.set_state("selected", json!(null));
    obj.methods.push(MethodSig::new("expand").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("collapse").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("select").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("refresh"));
    obj
}

/// 우측 캔버스 패널. 활성 파일 미리보기 또는 활성 앱 트리 슬롯.
///
/// state:
/// - `active_file: Option<ObjectId>` — 선택된 File 객체. 텍스트면 본문 미리보기 렌더.
/// - `active_app: Option<ObjectId>` — 캔버스에 mount된 앱의 루트 객체 (M8+ 메모장 등).
///   `active_app`이 Some이면 그쪽이 우선, `active_file`은 무시.
pub fn canvas(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Canvas@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_file", json!(null));
    obj.set_state("active_app", json!(null));
    obj.methods.push(MethodSig::new("set_file").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("clear_file"));
    obj.methods.push(MethodSig::new("set_app").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("clear_app"));
    obj
}

/// 폴더 노드. children에 Folder/File 객체.
///
/// props:
/// - `path: String` — 절대 경로
/// - `name: String` — 폴더명 (path의 basename)
///
/// state:
/// - `child_count: usize` — 자식 수 (UI 빠른 표시)
/// - `last_change_ms: i64` — 마지막 변경 Unix ms (자식 추가/제거 포함)
/// - `last_change_actor: String` — "ai" | "user" | "system"
pub fn folder(owner: ActorId, path: &str, name: &str, created_ms: i64) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Folder@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("path", json!(path));
    obj.set_prop("name", json!(name));
    obj.set_state("child_count", json!(0));
    obj.set_state("last_change_ms", json!(created_ms));
    obj.set_state("last_change_actor", json!("system"));
    obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("delete"));
    obj
}

/// 파일 노드.
///
/// props:
/// - `path: String` — 절대 경로
/// - `name: String` — 파일명
/// - `mime: String` — 추론된 MIME (확장자 기반)
///
/// state:
/// - `size_bytes: u64`
/// - `last_change_ms: i64`
/// - `last_change_actor: String`
/// - `preview: String` — 텍스트 파일에 한해 앞 512바이트, 그 외는 ""
pub fn file(owner: ActorId, path: &str, name: &str, mime: &str, created_ms: i64) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/File@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("path", json!(path));
    obj.set_prop("name", json!(name));
    obj.set_prop("mime", json!(mime));
    obj.set_state("size_bytes", json!(0u64));
    obj.set_state("last_change_ms", json!(created_ms));
    obj.set_state("last_change_actor", json!("system"));
    obj.set_state("preview", json!(""));
    obj.methods.push(MethodSig::new("read"));
    obj.methods.push(MethodSig::new("write").with_arg(ArgSpec::new("content", "string")));
    obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
    obj.methods.push(MethodSig::new("delete"));
    obj
}

// ───────────────────────── M7 T7.5: 하단 CLI 패널 ─────────────────────────

/// 하단 CLI 패널. 데스크톱 셸의 4번째 builtin (Desktop의 3번째 자식).
///
/// CLI는 *셸의 일급 구성요소* — 일반 명령 dispatch + (T7.7부터) AI 호출의 진입점.
/// bash/PowerShell처럼 모든 동적 명령 접근이 여기서 시작된다. ADR-023 참고.
///
/// state:
/// - `lines: [String]` — 출력 히스토리 (oldest first, ~1000 라인 cap은 호출자 책임)
/// - `history: [String]` — 입력 히스토리 (T7.5 v1은 빈 배열, ↑/↓ 네비는 deferred)
/// - `session_id: Option<String>` — AI 채팅 세션 ID (T7.7부터 사용, T7.5는 항상 null)
///
/// 메서드:
/// - `submit_input(text: String)` — 사용자가 Enter로 commit한 입력 라인. desktop-shell이
///   받아서 `cli_handler::dispatch_command`로 파싱·실행, 결과를 lines에 append.
/// - `clear()` — lines 비움 (입력 히스토리는 유지).
/// - `append_line(text: String)` — 외부(예: AI bridge)에서 출력 라인 추가.
///
/// 주의: `input_buffer`와 `cursor_pos`는 **컴포지터 local state**로 관리 — server tree에
/// 두지 않는다. 매 키 입력마다 invoke를 보내면 latency가 크기 때문. Cli 객체는 commit된
/// lines만 server에 보관한다.
pub fn cli(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Cli@1").expect("유효한 TypeUri"), owner);
    obj.set_state("lines", json!([] as [&str; 0]));
    obj.set_state("history", json!([] as [&str; 0]));
    obj.set_state("session_id", json!(null));
    obj.methods.push(MethodSig::new("submit_input").with_arg(ArgSpec::new("text", "string")));
    obj.methods.push(MethodSig::new("clear"));
    obj.methods.push(MethodSig::new("append_line").with_arg(ArgSpec::new("text", "string")));
    obj
}

// ───────────────────────── M8: 멀티-윈도우 탐색기 ─────────────────────────

/// 플로팅 파일 viewer 윈도우. Desktop의 자식으로 mount되어 오버레이로 떠있음.
///
/// props:
/// - `title: String` — 윈도우 상단 표시 (기본 = 파일명)
/// - `file_id: ObjectId` — 보여주는 File 객체
///
/// state:
/// - `x: i32`, `y: i32` — 좌상단 좌표
/// - `w: i32`, `h: i32` — 크기 (min 200×120)
/// - `z: i32` — z-order (큰 값이 위)
/// - `focused: bool` — 키보드 입력 수신 여부
///
/// 메서드: `move(x, y)`, `resize(w, h)`, `focus()`, `close()`
//
// 7개 인자는 (owner, title, file_id, x, y, w, h)로 전부 필수 — Window의 정체성과
// 초기 geometry를 한 번에 확정한다. 구조체로 묶으면 호출부 가독성이 오히려 떨어지므로
// 팩토리 시그니처는 그대로 두고 clippy 한정 허용.
#[allow(clippy::too_many_arguments)]
pub fn window(
    owner: ActorId,
    title: &str,
    file_id: ObjectId,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Window@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("title", json!(title));
    obj.set_prop("file_id", json!(file_id));
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.set_state("w", json!(w));
    obj.set_state("h", json!(h));
    obj.set_state("z", json!(0));
    obj.set_state("focused", json!(false));
    obj.methods.push(
        MethodSig::new("move")
            .with_arg(ArgSpec::new("x", "i32"))
            .with_arg(ArgSpec::new("y", "i32")),
    );
    obj.methods.push(
        MethodSig::new("resize")
            .with_arg(ArgSpec::new("w", "i32"))
            .with_arg(ArgSpec::new("h", "i32")),
    );
    obj.methods.push(MethodSig::new("focus"));
    obj.methods.push(MethodSig::new("close"));
    obj
}

/// 우측 파일 탐색기 패널. active_folder의 자식을 list로.
///
/// state:
/// - `active_folder: Option<ObjectId>` — 현재 표시 폴더. None이면 드라이브 일람 (FileTree root와 동일).
/// - `view_mode: String` — "list" (M8 고정). 향후 grid/details.
///
/// 메서드:
/// - `navigate_to(folder_id: ObjectId)` — 다른 폴더로 진입
/// - `open_file(file_id: ObjectId)` — 새 Window mount (이미 열려있으면 그것 focus)
pub fn explorer(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Explorer@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_folder", json!(null));
    obj.set_state("view_mode", json!("list"));
    obj.methods.push(MethodSig::new("navigate_to").with_arg(ArgSpec::new("folder_id", "ObjectId")));
    obj.methods.push(MethodSig::new("open_file").with_arg(ArgSpec::new("file_id", "ObjectId")));
    obj
}
