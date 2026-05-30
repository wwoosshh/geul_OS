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

/// 데스크톱 루트 셸. 바탕화면 + 떠있는 창 + 하단 CLI 호스트.
pub fn desktop(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Desktop@1").expect("유효한 TypeUri"), owner);
    obj.set_state("wallpaper", json!("#1E2A3A"));
    obj.set_state("cli_height", json!(220));
    obj.methods.push(MethodSig::new("launch").with_arg(ArgSpec::new("app", "string")));
    obj.methods.push(MethodSig::new("set_wallpaper").with_arg(ArgSpec::new("v", "string")));
    obj.methods.push(MethodSig::new("set_cli_height").with_arg(ArgSpec::new("px", "i32")));
    obj
}

/// 상단 네비게이션 바.
pub fn top_bar(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/TopBar@1").expect("유효한 TypeUri"), owner);
    obj.set_state("items", json!([{"id":"geulos","label":"GeulOS"}]));
    obj.set_state("clock", json!(""));
    obj.methods.push(MethodSig::new("activate").with_arg(ArgSpec::new("item_id", "string")));
    obj
}

/// 우측 퀵런치 독.
pub fn dock(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Dock@1").expect("유효한 TypeUri"), owner);
    obj.set_state("items", json!([]));
    obj.methods.push(MethodSig::new("launch").with_arg(ArgSpec::new("item_id", "string")));
    obj
}

/// 바탕화면 아이콘(다중). open()=해당 app 실행.
pub fn desktop_icon(owner: ActorId, app: &str, label: &str, icon: &str, x: i32, y: i32) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/DesktopIcon@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("app", json!(app));
    obj.set_prop("label", json!(label));
    obj.set_prop("icon", json!(icon));
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.methods.push(MethodSig::new("open"));
    obj
}

/// 파일관리자 창. FileTree+Explorer를 자식으로 감싸는 떠있는 창(Window 동형).
pub fn file_manager(owner: ActorId, x: i32, y: i32, w: i32, h: i32, z: i32) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/FileManager@1").expect("유효한 TypeUri"), owner);
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.set_state("w", json!(w));
    obj.set_state("h", json!(h));
    obj.set_state("z", json!(z));
    obj.set_state("focused", json!(true));
    obj.methods.push(MethodSig::new("move").with_arg(ArgSpec::new("x", "i32")).with_arg(ArgSpec::new("y", "i32")));
    obj.methods.push(MethodSig::new("resize").with_arg(ArgSpec::new("w", "i32")).with_arg(ArgSpec::new("h", "i32")));
    obj.methods.push(MethodSig::new("focus"));
    obj.methods.push(MethodSig::new("close"));
    obj
}

/// 좌측 파일 트리 패널.
///
/// props:
/// - `root_path: String` — 워크스페이스 절대 경로 (표시·디버그용)
///
/// state:
/// - `expanded: [ObjectId]` — 펼쳐진 폴더 ID 목록
/// - `selected: Option<ObjectId>` — 현재 선택된 노드 (FileTree 안에서 강조)
/// - `scroll_y: i32` — 첫 가시 라인 번호 (라인 단위, 기본 0). 컴포지터가 24px 곱해 픽셀
///   오프셋 계산. M8 T8.13 / ADR-033 — 세 영역(Window/FileTree/Explorer) 공통 스크롤.
///
/// 메서드: `expand(id)`, `collapse(id)`, `select(id)`, `refresh()` — 재스캔
pub fn file_tree(owner: ActorId, root_path: &str) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/FileTree@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("root_path", json!(root_path));
    obj.set_state("expanded", json!([] as [&str; 0]));
    obj.set_state("selected", json!(null));
    obj.set_state("scroll_y", json!(0));
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
///
/// **M8 read-only** (ADR-027) → M10 (ADR-036)에서 write 메서드 복귀.
///
/// 메서드: `create_file(name)`, `create_folder(name)`, `delete(recursive)`,
/// `rename(new_name)` — M10 / ADR-036.
pub fn folder(owner: ActorId, path: &str, name: &str, created_ms: i64) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Folder@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("path", json!(path));
    obj.set_prop("name", json!(name));
    obj.set_state("child_count", json!(0));
    obj.set_state("last_change_ms", json!(created_ms));
    obj.set_state("last_change_actor", json!("system"));
    // M10 Phase 1 (ADR-036): 객체-네이티브 파일시스템 메서드.
    // create_file/create_folder는 그 폴더 *안*에 새 파일/폴더를 만들고, 해당 객체 mount.
    // delete는 폴더 자체를 삭제 (recursive=true면 자식 포함).
    // rename은 폴더 자체의 이름 변경 — props.name + props.path 갱신.
    obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("delete").with_arg(ArgSpec::new("recursive", "bool")));
    obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
    // M10 Phase 2 후속: AI가 *expand되지 않은* 폴더의 내부 구조를 동적으로 인지하기 위한
    // 메서드. invoke 시 desktop-shell이 fs::read_dir로 직계 children을 mount + broadcast
    // (lazy_expand_if_needed와 같은 흐름). 사용자가 FileTree로 열어두지 않아도 AI는
    // list 호출로 즉시 자식 트리 인지.
    obj.methods.push(MethodSig::new("list"));
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
///
/// **M8 read-only** (ADR-027) → M9에서 `save` 복귀, M10 (ADR-036)에서 `delete`/`rename` 복귀.
///
/// 메서드: `read()`, `save(content)`, `delete()`, `rename(new_name)` — M10 / ADR-036.
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
    obj.methods.push(MethodSig::new("save").with_arg(ArgSpec::new("content", "string")));
    // M10 Phase 1 (ADR-036): 객체-네이티브 파일시스템 메서드.
    obj.methods.push(MethodSig::new("delete"));
    obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
    obj
}

// ───────────────────────── M7 T7.5: 하단 CLI 패널 ─────────────────────────

/// 하단 CLI 패널. 데스크톱 셸의 4번째 builtin (Desktop의 3번째 자식).
///
/// CLI는 *셸의 일급 구성요소* — 일반 명령 dispatch + AI 호출의 진입점.
/// bash/PowerShell처럼 모든 동적 명령 접근이 여기서 시작된다. ADR-023 참고.
///
/// state:
/// - `lines: [String]` — 출력 히스토리 (oldest first, ~1000 라인 cap은 호출자 책임)
/// - `history: [String]` — 입력 히스토리 (T7.5 v1은 빈 배열, ↑/↓ 네비는 deferred)
/// - `mode: String` — `"shell"` (기본) | `"ai"` (T7.8 / ADR-031) |
///   `"awaiting_api_key"` (T7.9 / ADR-032).
/// - `session_name: Option<String>` — AI 대화 모드에서 활성 세션 이름. 그 외 모드는 null.
/// - `pending_action: Option<String>` — T7.9 / ADR-032: awaiting_api_key 모드일 때 검증
///   성공 후 재실행할 액션 인코딩 (`"start"`, `"start NAME"`, `"load NAME"`). 그 외 null.
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
    // T7.8 / ADR-031: 명시적 chat mode + 활성 세션 이름. (T7.5의 placeholder `session_id`는
    // 의미 모호했으므로 제거하고 이 두 필드로 대체.)
    obj.set_state("mode", json!("shell"));
    obj.set_state("session_name", json!(null));
    // T7.9 / ADR-032: awaiting_api_key 모드에서 검증 성공 후 재실행할 명령. 그 외 null.
    obj.set_state("pending_action", json!(null));
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
/// - `scroll_y: i32` — 첫 가시 라인 번호 (라인 단위, 기본 0). M8 T8.13 / ADR-033.
/// - `content: String` — 파일 본문 (UTF-8, ≤ 1MB, 기본 ""). Window mount 직전
///   desktop-shell이 채움 — 별 `Notepad@1` 객체 X. 1MB cap은 *호출자 책임*.
/// - `content_too_large: bool` — 1MB cap에 걸려 잘렸으면 `true` (기본 `false`).
///   컴포지터가 "[일부만 표시]" 안내 렌더.
/// - `dirty: bool` — content가 디스크와 다르면 true (기본 `false`). M9 / ADR-035.
///
/// Window는 *항상 편집 가능* — viewer/editor 토글 없음 (메모장/notepad UX 통념).
/// focused일 때 키 입력이 곧 편집. dirty=true면 title에 `*` 표시.
///
/// 메서드: `move(x, y)`, `resize(w, h)`, `focus()`, `close()`,
///   `save_to_file(content)`, `close_confirm()` — M9 / ADR-035.
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
    obj.set_state("scroll_y", json!(0));
    obj.set_state("content", json!(""));
    obj.set_state("content_too_large", json!(false));
    obj.set_state("dirty", json!(false));
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
    // save_to_file에 args.content 포함 — compositor가 *local-master* content를 wire에 단 한 번
    // (Ctrl+S 시점)에 전달. v1에서 매 키마다 SetState(content)로 push하던 흐름은 큰 텍스트 파일에서
    // wire backpressure로 입력 freeze 유발 (사용자 보고) — content는 컴포지터 측에만 두고 save invoke
    // payload로만 보낸다.
    obj.methods.push(MethodSig::new("save_to_file").with_arg(ArgSpec::new("content", "string")));
    obj.methods.push(MethodSig::new("close_confirm"));
    obj
}

/// 우측 파일 탐색기 패널. active_folder의 자식을 list로.
///
/// state:
/// - `active_folder: Option<ObjectId>` — 현재 표시 폴더. None이면 드라이브 일람 (FileTree root와 동일).
/// - `view_mode: String` — "list" (M8 고정). 향후 grid/details.
/// - `scroll_y: i32` — 첫 가시 라인 번호 (라인 단위, 기본 0). 컴포지터가 24px 곱해 픽셀
///   오프셋 계산. M8 T8.13 / ADR-033 — 세 영역(Window/FileTree/Explorer) 공통 스크롤.
///
/// 메서드:
/// - `navigate_to(folder_id: ObjectId)` — 다른 폴더로 진입
/// - `navigate_up()` — active_folder의 부모로 이동. parent 없으면 드라이브 일람 reset.
/// - `open_file(file_id: ObjectId)` — 새 Window mount (이미 열려있으면 그것 focus)
pub fn explorer(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Explorer@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_folder", json!(null));
    obj.set_state("view_mode", json!("list"));
    obj.set_state("scroll_y", json!(0));
    obj.methods.push(MethodSig::new("navigate_to").with_arg(ArgSpec::new("folder_id", "ObjectId")));
    obj.methods.push(MethodSig::new("navigate_up"));
    obj.methods.push(MethodSig::new("open_file").with_arg(ArgSpec::new("file_id", "ObjectId")));
    obj.set_state("selected_item", json!(null));
    obj.methods.push(MethodSig::new("select").with_arg(ArgSpec::new("folder_id", "string")));
    obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("rename_selected").with_arg(ArgSpec::new("new_name", "string")));
    obj.methods.push(MethodSig::new("delete_selected"));
    obj
}

// ───────────────────────── M9: Dialog@1 (modal confirm/warn) ─────────────────────────

/// 모달 다이얼로그. Desktop의 자식으로 mount되어 z-최상위 오버레이로 떠있음.
///
/// props:
/// - `title: String`
/// - `message: String`
/// - `kind: String` — `"confirm"` | `"warn"`
/// - `actions: [String]` — 버튼 라벨 배열 (예: `["허용", "거부"]`).
///
/// state:
/// - `result: Option<String>` — 사용자가 클릭한 action 라벨. null=pending.
///
/// 메서드: `respond(action: String)` — compositor가 사용자 클릭 결과 전달.
///
/// Modal: compositor가 layout에서 *항상 z-최상위*로 push하고, hit_test가 Dialog 떠있을 때
/// Dialog rect 밖 클릭을 *consume(무시)*하여 다른 Window/CLI/Explorer 입력을 block.
pub fn dialog(
    owner: ActorId,
    title: &str,
    message: &str,
    kind: &str,
    actions: Vec<String>,
) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Dialog@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("title", json!(title));
    obj.set_prop("message", json!(message));
    obj.set_prop("kind", json!(kind));
    obj.set_prop("actions", json!(actions));
    obj.set_state("result", json!(null));
    obj.methods.push(MethodSig::new("respond").with_arg(ArgSpec::new("action", "string")));
    obj
}

// ───────────────────────── M10 Phase 3: Filesystem@1 escape hatch ─────────────────────────

/// cwd 밖 임의 path 접근용 singleton (M10 Phase 3 / ADR-036). desktop-shell이 시작 시 한 번
/// mount.
///
/// props:
/// - `root_path: String` — 현재 cwd (참조용)
///
/// state:
/// - `granted_dirs: [String]` — 시각 표시용 (실제 정책은 desktop-shell GrantedDirs)
/// - `last_read_path: Option<String>` — `read_external` 직후 desktop-shell이 SetState로 채움
/// - `last_read_content: Option<String>` — 같음. AI는 get_object로 본문 확인.
///
/// 메서드:
/// - `read_external(path: String) -> String` — cwd 밖 path read. cwd 안 호출은 거부.
/// - `write_external(path: String, content: String)` — cwd 밖 path write. 매번 Dialog confirm.
///
/// **cwd 안 호출은 거부**: 객체-네이티브 흐름 (Folder/File 메서드) 우선. AI가 cwd 안 path를
/// `*_external`로 호출하면 desktop-shell이 거부 + 안내 메시지 (Folder@1/File@1 사용 권장).
pub fn filesystem(owner: ActorId, root_path: &str) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Filesystem@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("root_path", json!(root_path));
    obj.set_state("granted_dirs", json!([] as [&str; 0]));
    obj.set_state("last_read_path", json!(null));
    obj.set_state("last_read_content", json!(null));
    // ai-bridge mutation polling이 ready 신호로 사용 — 각 mutation 완료 시 path SetState.
    obj.set_state("last_write_path", json!(null));
    obj.set_state("last_delete_path", json!(null));
    obj.set_state("last_rename_from_path", json!(null));
    obj.set_state("last_rename_to_path", json!(null));
    obj.methods.push(MethodSig::new("read_external").with_arg(ArgSpec::new("path", "string")));
    obj.methods.push(
        MethodSig::new("write_external")
            .with_arg(ArgSpec::new("path", "string"))
            .with_arg(ArgSpec::new("content", "string")),
    );
    obj.methods.push(MethodSig::new("delete_external").with_arg(ArgSpec::new("path", "string")));
    obj.methods.push(
        MethodSig::new("rename_external")
            .with_arg(ArgSpec::new("from", "string"))
            .with_arg(ArgSpec::new("to", "string")),
    );
    obj
}

// ───────────────────────── M12: ShellRunner@1 escape hatch ─────────────────────────

/// `aios.builtin/ShellRunner@1` 객체 (M12) — 화이트리스트 binary 실행 escape hatch.
///
/// Filesystem@1과 같은 singleton 패턴. 임의 명령이 아닌 *허용된 binary*만 통과
/// (props.allowed_binaries — 사용자가 mount 시점 또는 SetState로 확장 가능).
///
/// method `run(cmd, args, cwd)` — desktop-shell handler가 화이트리스트 + cwd 검증
/// 후 AI sender면 Dialog 흐름, compositor면 즉시 실행. 결과는 state.last_* 8 fields
/// SetState. 본 v1은 one-shot 명령만 (wait_with_output). long-running은 M13+
/// Process@1 별도.
pub fn shellrunner(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/ShellRunner@1").expect("유효한 TypeUri"), owner);
    obj.set_prop(
        "allowed_binaries",
        json!([
            "git", "npm", "yarn", "pnpm", "npx", "cargo", "rustc", "docker", "node", "python",
            "pip"
        ]),
    );
    obj.set_prop("default_timeout_ms", json!(120000u64));
    for k in &[
        "last_cmd",
        "last_args",
        "last_cwd",
        "last_exit_code",
        "last_stdout",
        "last_stderr",
        "last_duration_ms",
        "last_error",
    ] {
        obj.set_state(*k, json!(null));
    }
    obj.methods.push(
        MethodSig::new("run")
            .with_arg(ArgSpec::new("cmd", "string"))
            .with_arg(ArgSpec::new("args", "array<string>"))
            .with_arg(ArgSpec::new("cwd", "string")),
    );
    // M13 — long-running 명령 (dev server 등). 결과는 ConsoleWindow@1 mount.
    obj.methods.push(
        MethodSig::new("run_streamed")
            .with_arg(ArgSpec::new("cmd", "string"))
            .with_arg(ArgSpec::new("args", "array<string>"))
            .with_arg(ArgSpec::new("cwd", "string")),
    );
    obj
}

// ───────────────────────── M13: ConsoleWindow@1 long-running process ─────────────────────────

/// `aios.builtin/ConsoleWindow@1` 객체 (M13) — long-running process 시각화 + 제어.
///
/// ShellRunner.run_streamed가 결과로 mount. Window@1-유사 floating panel UI.
/// stdout/stderr가 state.lines (ring max 500)에 line별 SetState로 stream.
/// terminate() 또는 사용자 X 닫기 = Windows JobObject TerminateJobObject로
/// 손주 process까지 cascade kill (npm.cmd → node → esbuild 사슬).
///
/// methods:
/// - `terminate()` — 사용자/AI 호출 (AI는 desktop-shell handler가 Dialog mount).
/// - `close()` — compositor의 X 클릭 hook. handler가 terminate로 위임.
/// - `move(x, y)` / `resize(w, h)` / `focus()` — Window@1과 동일 UI 메서드.
/// - `scroll(y)` — 본문 viewport scroll 위치 (UI 전용).
#[allow(clippy::too_many_arguments)]
pub fn console_window(
    owner: ActorId,
    cmd: String,
    args: Vec<String>,
    cwd: String,
    title: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/ConsoleWindow@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("cmd", json!(cmd));
    obj.set_prop("args", json!(args));
    obj.set_prop("cwd", json!(cwd));
    obj.set_prop("title", json!(title));
    obj.set_state("pid", json!(null));
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.set_state("w", json!(w));
    obj.set_state("h", json!(h));

    obj.set_state("lines", json!([] as [&str; 0]));
    obj.set_state("line_count", json!(0u64));
    obj.set_state("status", json!("running"));
    obj.set_state("exit_code", json!(null));
    obj.set_state("started_at", json!(chrono::Utc::now().to_rfc3339()));
    obj.set_state("ended_at", json!(null));
    obj.set_state("scroll_y", json!(0));

    obj.methods.push(MethodSig::new("terminate"));
    obj.methods.push(MethodSig::new("close"));
    obj.methods.push(MethodSig::new("focus"));
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
    obj.methods.push(MethodSig::new("scroll").with_arg(ArgSpec::new("y", "i32")));
    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_has_save_method() {
        let f = file(ActorId::local_user(), "/x.txt", "x.txt", "text/plain", 0);
        assert!(f.methods.iter().any(|m| m.name() == "save"));
    }

    #[test]
    fn window_has_dirty_state_and_save_methods() {
        let w = window(ActorId::local_user(), "t", ObjectId::new(), 0, 0, 600, 400);
        assert_eq!(w.state.get("dirty"), Some(&json!(false)));
        // edit_mode 제거 — Window는 항상 편집 가능 (메모장 UX).
        assert!(!w.state.contains_key("edit_mode"));
        // toggle_edit 제거. save_to_file/close_confirm 존재.
        assert!(!w.methods.iter().any(|m| m.name() == "toggle_edit"));
        assert!(w.methods.iter().any(|m| m.name() == "save_to_file"));
        assert!(w.methods.iter().any(|m| m.name() == "close_confirm"));
    }

    #[test]
    fn folder_has_fs_methods() {
        let f = folder(ActorId::local_user(), "/p", "p", 0);
        assert!(f.methods.iter().any(|m| m.name() == "create_file"));
        assert!(f.methods.iter().any(|m| m.name() == "create_folder"));
        assert!(f.methods.iter().any(|m| m.name() == "delete"));
        assert!(f.methods.iter().any(|m| m.name() == "rename"));
    }

    #[test]
    fn file_has_fs_methods() {
        let f = file(ActorId::local_user(), "/x.txt", "x.txt", "text/plain", 0);
        assert!(f.methods.iter().any(|m| m.name() == "delete"));
        assert!(f.methods.iter().any(|m| m.name() == "rename"));
    }

    #[test]
    fn filesystem_factory_singleton() {
        let fs = filesystem(ActorId::local_user(), "D:/GeulOS");
        assert_eq!(fs.type_uri.as_str(), "aios.builtin/Filesystem@1");
        assert_eq!(fs.props.get("root_path"), Some(&json!("D:/GeulOS")));
        assert_eq!(fs.state.get("granted_dirs"), Some(&json!([] as [&str; 0])));
        assert_eq!(fs.state.get("last_read_path"), Some(&json!(null)));
        assert_eq!(fs.state.get("last_read_content"), Some(&json!(null)));
        assert!(fs.methods.iter().any(|m| m.name() == "read_external"));
        assert!(fs.methods.iter().any(|m| m.name() == "write_external"));
    }

    #[test]
    fn shellrunner_has_run_method_and_state() {
        let sr = shellrunner(ActorId::local_user());
        assert_eq!(sr.type_uri.as_str(), "aios.builtin/ShellRunner@1");
        assert!(sr.methods.iter().any(|m| m.name() == "run"));
        assert!(sr.methods.iter().any(|m| m.name() == "run_streamed"));
        assert!(sr.props.contains_key("allowed_binaries"));
        assert!(sr.props.contains_key("default_timeout_ms"));
        let allowed = sr.props.get("allowed_binaries").and_then(|v| v.as_array()).unwrap();
        assert!(allowed.iter().any(|v| v.as_str() == Some("git")));
        assert!(allowed.iter().any(|v| v.as_str() == Some("npm")));
        assert!(allowed.iter().any(|v| v.as_str() == Some("cargo")));
        for k in &[
            "last_cmd",
            "last_args",
            "last_cwd",
            "last_exit_code",
            "last_stdout",
            "last_stderr",
            "last_duration_ms",
            "last_error",
        ] {
            assert!(sr.state.contains_key(*k), "state.{} 누락", k);
            assert_eq!(sr.state.get(*k), Some(&serde_json::json!(null)), "state.{} 초기 null", k);
        }
    }

    #[test]
    fn console_window_factory_creates_with_props_state_methods() {
        let cw = console_window(
            ActorId::local_user(),
            "npm".to_string(),
            vec!["run".to_string(), "dev".to_string()],
            "D:/proj".to_string(),
            "npm run dev — proj".to_string(),
            100,
            100,
            800,
            600,
        );
        assert_eq!(cw.type_uri.as_str(), "aios.builtin/ConsoleWindow@1");

        // props 불변
        assert_eq!(cw.props.get("cmd"), Some(&serde_json::json!("npm")));
        assert_eq!(cw.props.get("args"), Some(&serde_json::json!(["run", "dev"])));
        assert_eq!(cw.props.get("cwd"), Some(&serde_json::json!("D:/proj")));
        assert_eq!(cw.props.get("title"), Some(&serde_json::json!("npm run dev — proj")));
        // geometry + pid는 state (move/resize/spawn으로 동적 변경 가능)
        assert_eq!(cw.state.get("x"), Some(&serde_json::json!(100)));
        assert_eq!(cw.state.get("y"), Some(&serde_json::json!(100)));
        assert_eq!(cw.state.get("w"), Some(&serde_json::json!(800)));
        assert_eq!(cw.state.get("h"), Some(&serde_json::json!(600)));
        assert_eq!(cw.state.get("pid"), Some(&serde_json::json!(null)));

        // state 초기값
        assert_eq!(cw.state.get("lines"), Some(&serde_json::json!([] as [&str; 0])));
        assert_eq!(cw.state.get("line_count"), Some(&serde_json::json!(0u64)));
        assert_eq!(cw.state.get("status"), Some(&serde_json::json!("running")));
        assert_eq!(cw.state.get("exit_code"), Some(&serde_json::json!(null)));
        assert_eq!(cw.state.get("ended_at"), Some(&serde_json::json!(null)));
        assert_eq!(cw.state.get("scroll_y"), Some(&serde_json::json!(0)));
        assert!(cw.state.contains_key("started_at"));

        // methods
        for m in &["terminate", "close", "focus", "move", "resize", "scroll"] {
            assert!(cw.methods.iter().any(|x| x.name() == *m), "method {} 누락", m);
        }
    }

    #[test]
    fn explorer_v1_5_methods_and_state() {
        let ex = explorer(ActorId::local_user());
        let names: Vec<&str> = ex.methods.iter().map(|m| m.name()).collect();
        assert!(names.contains(&"select"));
        assert!(names.contains(&"create_file"));
        assert!(names.contains(&"create_folder"));
        assert!(names.contains(&"rename_selected"));
        assert!(names.contains(&"delete_selected"));
        assert_eq!(ex.state.get("selected_item"), Some(&json!(null)));
    }

    #[test]
    fn dialog_factory_sets_props_state_methods() {
        let d = dialog(
            ActorId::local_user(),
            "확인",
            "정말?",
            "confirm",
            vec!["허용".to_string(), "거부".to_string()],
        );
        assert_eq!(d.type_uri.as_str(), "aios.builtin/Dialog@1");
        assert_eq!(d.props.get("title"), Some(&json!("확인")));
        assert_eq!(d.props.get("actions"), Some(&json!(["허용", "거부"])));
        assert_eq!(d.state.get("result"), Some(&json!(null)));
        assert!(d.methods.iter().any(|m| m.name() == "respond"));
    }
}

#[cfg(test)]
mod sp1_tests {
    use super::*;

    fn owner() -> ActorId {
        ActorId::local_user()
    }

    #[test]
    fn factories_have_expected_types_state_methods() {
        // TopBar
        let o = top_bar(owner());
        assert_eq!(o.type_uri.as_str(), "aios.builtin/TopBar@1");
        assert!(o.methods.iter().any(|m| m.name() == "activate"));

        // Dock
        let d = dock(owner());
        assert_eq!(d.type_uri.as_str(), "aios.builtin/Dock@1");
        assert!(d.methods.iter().any(|m| m.name() == "launch"));

        // DesktopIcon
        let ic = desktop_icon(owner(), "file_manager", "파일관리자", "folder", 40, 40);
        assert_eq!(ic.type_uri.as_str(), "aios.builtin/DesktopIcon@1");
        assert_eq!(ic.props.get("app"), Some(&json!("file_manager")));
        assert_eq!(ic.props.get("label"), Some(&json!("파일관리자")));
        assert_eq!(ic.props.get("icon"), Some(&json!("folder")));
        assert_eq!(ic.state.get("x"), Some(&json!(40)));
        assert_eq!(ic.state.get("y"), Some(&json!(40)));
        assert!(ic.methods.iter().any(|m| m.name() == "open"));

        // FileManager
        let fm = file_manager(owner(), 100, 80, 700, 460, 1);
        assert_eq!(fm.type_uri.as_str(), "aios.builtin/FileManager@1");
        assert_eq!(fm.state.get("x"), Some(&json!(100)));
        assert_eq!(fm.state.get("y"), Some(&json!(80)));
        assert_eq!(fm.state.get("w"), Some(&json!(700)));
        assert_eq!(fm.state.get("h"), Some(&json!(460)));
        assert_eq!(fm.state.get("z"), Some(&json!(1)));
        assert_eq!(fm.state.get("focused"), Some(&json!(true)));
        for m_name in ["move", "resize", "focus", "close"] {
            assert!(fm.methods.iter().any(|m| m.name() == m_name), "method {} 누락", m_name);
        }

        // Desktop (extended)
        let desk = desktop(owner());
        assert_eq!(desk.type_uri.as_str(), "aios.builtin/Desktop@1");
        assert_eq!(desk.state.get("wallpaper"), Some(&json!("#1E2A3A")));
        assert_eq!(desk.state.get("cli_height"), Some(&json!(220)));
        assert!(desk.methods.iter().any(|m| m.name() == "launch"));
        assert!(desk.methods.iter().any(|m| m.name() == "set_wallpaper"));
        assert!(desk.methods.iter().any(|m| m.name() == "set_cli_height"));
    }
}
