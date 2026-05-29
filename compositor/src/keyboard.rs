//! 키보드 입력 → 컴포지터 local CLI 상태 변환.
//!
//! 컴포지터는 키 입력을 받아 *서버로 즉시 invoke를 보내지 않는다*. 매 키마다 RPC를 보내면
//! latency가 크기 때문. 입력 버퍼와 커서 위치는 컴포지터 local state로 유지하고,
//! Enter(submit) 시점에만 `submit_input` invoke를 보낸다.
//!
//! T7.5: 영문/숫자/공백 + Backspace + Enter (ASCII v1).
//! T7.6 (ADR-029): winit `WindowEvent::Ime`로 한글 IME 위임 — `preedit_text`(조합 중)와
//! `handle_ime_commit`(조합 완료) 추가. preedit는 server에 절대 전송되지 않고 컴포지터
//! local로만 살아 있다가 commit 시점에 `input_buffer`로 흡수된다.

/// 컴포지터-사이드 CLI 상태. winit 메인 스레드 안에서만 mutate된다.
#[derive(Debug, Default, Clone)]
pub struct CliLocalState {
    /// 현재 편집 중인 입력 라인 (UTF-8 문자열).
    pub input_buffer: String,
    /// `input_buffer` 안 byte index 커서 위치. T7.6부터 한글이 들어올 수 있으므로
    /// 항상 char boundary를 유지해야 한다 (handle_key의 Backspace + handle_ime_commit의
    /// insert_str가 모두 이를 보장).
    pub cursor_pos: usize,
    /// T7.6: IME 조합 중 텍스트 (commit 전). server에는 절대 전송되지 않고 화면에만
    /// 회색으로 시각화된다. winit `Ime::Preedit` 도착 시 갱신, `Ime::Commit`/`Ime::Disabled`
    /// 시 비워진다.
    pub preedit_text: String,
    /// CLI 출력 라인의 bottom 기준 scroll offset (라인 단위).
    /// 0 = 가장 최신 라인이 입력 라인 바로 위에 보임 (기본). N > 0 = N 라인 위로 스크롤 —
    /// 이전 출력 확인. PageUp/PageDown/마우스 휠로 조정. submit 시 0으로 reset.
    pub scroll_offset: usize,
    /// SP4: 텍스트 영역 선택의 anchor (byte offset, 항상 char boundary). 선택은
    /// (min(anchor, cursor_pos), max(anchor, cursor_pos))이며, anchor==cursor_pos면 선택 없음.
    /// 마우스 드래그 시작 시 set, 타이핑/클릭/Esc로 해제. None이면 선택 없음.
    pub selection_anchor: Option<usize>,
}

impl CliLocalState {
    /// 키 입력 한 건을 처리한다. Enter면 Some(submitted_text)를 반환하고 버퍼는 비운다.
    /// 그 외엔 None — 호출자가 redraw만 요청.
    pub fn handle_key(&mut self, action: KeyAction) -> Option<String> {
        match action {
            KeyAction::InsertChar(c) => {
                // ASCII 가시 문자 + 공백만 허용. 제어문자는 무시 (Tab 등은 v2).
                if c == ' ' || (c.is_ascii_graphic() && !c.is_control()) {
                    // 선택이 있으면 덮어쓰기 — 선택 지우고 그 자리에 삽입.
                    self.delete_selection();
                    let mut tmp = String::with_capacity(4);
                    tmp.push(c);
                    self.input_buffer.insert_str(self.cursor_pos, &tmp);
                    self.cursor_pos += tmp.len();
                }
                None
            }
            KeyAction::Backspace => {
                // 선택이 있으면 선택만 삭제 (표준 동작).
                if self.delete_selection() {
                    return None;
                }
                if self.cursor_pos > 0 {
                    // ASCII만이므로 char 한 칸 = byte 한 칸. 그러나 UTF-8 안전성을 위해
                    // char boundary를 찾아서 삭제 — 후속 T7.6에서 한글이 들어와도 OK.
                    let new_pos = prev_char_boundary(&self.input_buffer, self.cursor_pos);
                    self.input_buffer.replace_range(new_pos..self.cursor_pos, "");
                    self.cursor_pos = new_pos;
                }
                None
            }
            KeyAction::Submit => {
                // 새 입력 commit 시 자동 scroll-to-bottom — 사용자가 위로 스크롤해 있어도
                // 자신의 입력 결과는 즉시 보여야 자연.
                self.scroll_offset = 0;
                self.selection_anchor = None;
                if self.input_buffer.is_empty() {
                    return Some(String::new());
                }
                let out = std::mem::take(&mut self.input_buffer);
                self.cursor_pos = 0;
                Some(out)
            }
        }
    }

    /// T7.6: winit `Ime::Preedit` 이벤트 — 조합 중 텍스트 갱신 (commit 전).
    ///
    /// `input_buffer`와 `cursor_pos`는 *건드리지 않는다* — preedit는 화면에만 회색으로
    /// 별도 표시되고 server에는 절대 전송되지 않는다. winit이 빈 문자열을 보내면
    /// `preedit_text`가 자연스럽게 비워진다 (Disabled 직전에도 같은 동작).
    pub fn handle_ime_preedit(&mut self, text: String) {
        self.preedit_text = text;
    }

    /// T7.6: winit `Ime::Commit` 이벤트 — 조합 완료 텍스트를 `input_buffer`에 삽입한다.
    ///
    /// `cursor_pos`는 항상 char boundary에 있고 `String::insert_str`는 byte offset 기반
    /// 이지만 char boundary에서는 multi-byte 한글도 안전하게 삽입된다. 삽입 후 cursor를
    /// `text.len()`(byte length)만큼 전진시켜 새 char boundary로 이동시킨다.
    pub fn handle_ime_commit(&mut self, text: &str) {
        // 선택이 있으면 먼저 덮어쓰기 (한글로 선택 영역 교체).
        self.delete_selection();
        self.input_buffer.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.preedit_text.clear();
    }

    /// T7.10: 클립보드 paste — `text`를 현재 cursor 위치에 그대로 삽입.
    ///
    /// `handle_ime_commit`과 같은 자리(cursor_pos 기준 insert_str + byte-len 전진)에
    /// 작동하므로 multi-byte 안전. 줄바꿈/탭 등 제어문자도 *필터링 없이 그대로* 삽입 —
    /// v1은 단순 (Anthropic API key가 한 줄이라 실용상 OK). v2에서 submit 시점에
    /// `\n` 첫 줄만 사용하는 정제 로직 추가 검토.
    ///
    /// 호출자(`compositor::main`)는 *focus=Cli일 때만* 본 메서드를 호출 — Window/None
    /// focus에서는 paste 무시 (M8 read-only Window 본문과 일관).
    pub fn handle_paste(&mut self, text: &str) {
        // 선택이 있으면 먼저 지우고 그 자리에 삽입 (덮어쓰기) — 표준 에디터 동작.
        self.delete_selection();
        self.input_buffer.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
    }

    // ── SP4: 텍스트 영역 선택 ──────────────────────────────────────────────────

    /// 현재 선택 범위를 정규화한 (start, end) byte offset. 선택 없으면 None.
    /// anchor==cursor(빈 선택)도 None.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        let (s, e) = if anchor <= self.cursor_pos { (anchor, self.cursor_pos) } else { (self.cursor_pos, anchor) };
        if s == e {
            None
        } else {
            Some((s.min(self.input_buffer.len()), e.min(self.input_buffer.len())))
        }
    }

    /// 선택된 텍스트. 선택 없으면 None.
    pub fn selected_text(&self) -> Option<String> {
        let (s, e) = self.selection_range()?;
        Some(self.input_buffer[s..e].to_string())
    }

    /// 마우스 press: 선택 anchor를 offset에 놓고 cursor도 거기로 (빈 선택 시작).
    pub fn start_selection_at(&mut self, offset: usize) {
        let off = offset.min(self.input_buffer.len());
        self.selection_anchor = Some(off);
        self.cursor_pos = off;
    }

    /// 마우스 drag: cursor를 offset으로 이동 (anchor 유지 → 선택 확장).
    pub fn extend_selection_to(&mut self, offset: usize) {
        self.cursor_pos = offset.min(self.input_buffer.len());
    }

    /// 전체 선택 (Ctrl+A). 빈 버퍼면 무동작.
    pub fn select_all(&mut self) {
        if self.input_buffer.is_empty() {
            self.selection_anchor = None;
            return;
        }
        self.selection_anchor = Some(0);
        self.cursor_pos = self.input_buffer.len();
    }

    /// 선택 해제 (anchor 비움). cursor는 유지.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// 선택 영역을 지운다. cursor를 선택 시작으로. 선택이 있었으면 true.
    pub fn delete_selection(&mut self) -> bool {
        if let Some((s, e)) = self.selection_range() {
            self.input_buffer.replace_range(s..e, "");
            self.cursor_pos = s;
            self.selection_anchor = None;
            true
        } else {
            self.selection_anchor = None;
            false
        }
    }

    /// 복사 (Ctrl+C): 선택 텍스트 반환. 버퍼·선택은 그대로.
    pub fn copy_selection(&self) -> Option<String> {
        self.selected_text()
    }

    /// 잘라내기 (Ctrl+X): 선택 텍스트 반환 + 삭제.
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        Some(text)
    }
}

/// 컴포지터 main이 KeyboardInput을 분석해 만들어내는 의미적 액션.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// ASCII 가시 문자 삽입 (공백 포함).
    InsertChar(char),
    /// Backspace — 커서 직전 char 삭제.
    Backspace,
    /// Enter — 현재 input_buffer를 submit_input으로 commit.
    Submit,
}

/// `s`의 `idx` 직전 char 경계의 byte index를 반환. idx==0이면 0.
fn prev_char_boundary(s: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_ascii_appends_to_buffer() {
        let mut s = CliLocalState::default();
        assert_eq!(s.handle_key(KeyAction::InsertChar('h')), None);
        assert_eq!(s.handle_key(KeyAction::InsertChar('i')), None);
        assert_eq!(s.input_buffer, "hi");
        assert_eq!(s.cursor_pos, 2);
    }

    #[test]
    fn insert_space_allowed() {
        let mut s = CliLocalState::default();
        s.handle_key(KeyAction::InsertChar('a'));
        s.handle_key(KeyAction::InsertChar(' '));
        s.handle_key(KeyAction::InsertChar('b'));
        assert_eq!(s.input_buffer, "a b");
    }

    #[test]
    fn insert_control_char_ignored() {
        let mut s = CliLocalState::default();
        s.handle_key(KeyAction::InsertChar('\t'));
        s.handle_key(KeyAction::InsertChar('\n'));
        assert_eq!(s.input_buffer, "");
    }

    #[test]
    fn backspace_deletes_last_char() {
        let mut s = CliLocalState::default();
        s.handle_key(KeyAction::InsertChar('a'));
        s.handle_key(KeyAction::InsertChar('b'));
        s.handle_key(KeyAction::Backspace);
        assert_eq!(s.input_buffer, "a");
        assert_eq!(s.cursor_pos, 1);
    }

    #[test]
    fn backspace_at_empty_is_noop() {
        let mut s = CliLocalState::default();
        assert_eq!(s.handle_key(KeyAction::Backspace), None);
        assert_eq!(s.input_buffer, "");
        assert_eq!(s.cursor_pos, 0);
    }

    #[test]
    fn submit_returns_buffer_and_clears() {
        let mut s = CliLocalState::default();
        s.handle_key(KeyAction::InsertChar('h'));
        s.handle_key(KeyAction::InsertChar('i'));
        let out = s.handle_key(KeyAction::Submit);
        assert_eq!(out, Some("hi".to_string()));
        assert_eq!(s.input_buffer, "");
        assert_eq!(s.cursor_pos, 0);
    }

    #[test]
    fn submit_empty_returns_empty_string() {
        let mut s = CliLocalState::default();
        let out = s.handle_key(KeyAction::Submit);
        // 빈 입력도 호출자에게 알려줘 dispatch 함수가 빈 outcome 반환하도록 함.
        assert_eq!(out, Some(String::new()));
    }

    // T7.6 (ADR-029): IME 단위 테스트.

    #[test]
    fn ime_preedit_stores_text_without_modifying_input() {
        // preedit는 화면 시각화 전용 — input_buffer/cursor_pos는 손대지 않는다.
        let mut state = CliLocalState {
            input_buffer: "hello".to_string(),
            cursor_pos: 5,
            ..Default::default()
        };
        state.handle_ime_preedit("ㅎㅏ".to_string());
        assert_eq!(state.preedit_text, "ㅎㅏ");
        assert_eq!(state.input_buffer, "hello");
        assert_eq!(state.cursor_pos, 5);
    }

    #[test]
    fn ime_commit_inserts_at_cursor_and_clears_preedit() {
        // commit은 input_buffer로 흡수 + cursor 전진 + preedit 비움.
        let mut state = CliLocalState {
            input_buffer: "abc".to_string(),
            cursor_pos: 3,
            preedit_text: "조합중".to_string(),
            ..Default::default()
        };
        state.handle_ime_commit("한글");
        assert_eq!(state.input_buffer, "abc한글");
        // "한글"은 UTF-8 6 bytes. cursor는 3 + 6 = 9.
        assert_eq!(state.cursor_pos, 3 + "한글".len());
        assert_eq!(state.preedit_text, "");
    }

    #[test]
    fn ime_commit_in_middle_of_buffer() {
        // cursor가 char boundary면 multi-byte 삽입도 안전.
        let mut state =
            CliLocalState { input_buffer: "ab".to_string(), cursor_pos: 1, ..Default::default() };
        state.handle_ime_commit("한");
        assert_eq!(state.input_buffer, "a한b");
        assert_eq!(state.cursor_pos, 1 + "한".len());
    }

    #[test]
    fn ime_preedit_empty_string_clears_buffer() {
        // winit이 빈 문자열을 보내면(조합 취소 / Disabled 직전) preedit가 비워진다.
        let mut state = CliLocalState { preedit_text: "ㅎ".to_string(), ..Default::default() };
        state.handle_ime_preedit(String::new());
        assert_eq!(state.preedit_text, "");
    }

    // T7.10: 클립보드 paste 단위 테스트.

    #[test]
    fn paste_inserts_text_at_cursor() {
        let mut state =
            CliLocalState { input_buffer: "ab".to_string(), cursor_pos: 1, ..Default::default() };
        state.handle_paste("XYZ");
        assert_eq!(state.input_buffer, "aXYZb");
        assert_eq!(state.cursor_pos, 1 + 3);
    }

    #[test]
    fn paste_at_end_appends() {
        let mut state = CliLocalState {
            input_buffer: "hello".to_string(),
            cursor_pos: 5,
            ..Default::default()
        };
        state.handle_paste(" world");
        assert_eq!(state.input_buffer, "hello world");
        assert_eq!(state.cursor_pos, 11);
    }

    #[test]
    fn paste_unicode_safe() {
        // cursor가 char boundary면 multi-byte UTF-8 삽입도 안전 (ime_commit_in_middle_of_buffer
        // 와 같은 invariant). API key는 ASCII지만 사용자가 한글 메모를 paste할 수도 있어 검증.
        let mut state =
            CliLocalState { input_buffer: "ab".to_string(), cursor_pos: 2, ..Default::default() };
        state.handle_paste("한글");
        assert_eq!(state.input_buffer, "ab한글");
        assert_eq!(state.cursor_pos, 2 + "한글".len());
    }

    // ── SP4: 영역 선택 / 복사 / 잘라내기 / 붙여넣기 ─────────────────────────────

    fn st(buf: &str, cursor: usize, anchor: Option<usize>) -> CliLocalState {
        CliLocalState {
            input_buffer: buf.to_string(),
            cursor_pos: cursor,
            selection_anchor: anchor,
            ..Default::default()
        }
    }

    #[test]
    fn selection_range_normalizes_order() {
        // anchor가 cursor보다 뒤여도 (start, end)로 정규화.
        let s = st("hello", 1, Some(4));
        assert_eq!(s.selection_range(), Some((1, 4)));
        let s = st("hello", 4, Some(1));
        assert_eq!(s.selection_range(), Some((1, 4)));
    }

    #[test]
    fn empty_selection_is_none() {
        // anchor==cursor면 선택 없음.
        let s = st("hello", 2, Some(2));
        assert_eq!(s.selection_range(), None);
        // anchor None이면 선택 없음.
        let s = st("hello", 2, None);
        assert_eq!(s.selection_range(), None);
    }

    #[test]
    fn selected_text_returns_substring() {
        let s = st("hello", 1, Some(4));
        assert_eq!(s.selected_text(), Some("ell".to_string()));
    }

    #[test]
    fn select_all_spans_buffer() {
        let mut s = st("hi한", 0, None);
        s.select_all();
        assert_eq!(s.selection_anchor, Some(0));
        assert_eq!(s.cursor_pos, "hi한".len());
        assert_eq!(s.selected_text(), Some("hi한".to_string()));
    }

    #[test]
    fn select_all_empty_buffer_no_selection() {
        let mut s = st("", 0, None);
        s.select_all();
        assert_eq!(s.selection_range(), None);
    }

    #[test]
    fn delete_selection_removes_and_collapses_cursor() {
        let mut s = st("hello", 1, Some(4));
        assert!(s.delete_selection());
        assert_eq!(s.input_buffer, "ho");
        assert_eq!(s.cursor_pos, 1);
        assert_eq!(s.selection_anchor, None);
    }

    #[test]
    fn delete_selection_no_selection_returns_false() {
        let mut s = st("hello", 2, None);
        assert!(!s.delete_selection());
        assert_eq!(s.input_buffer, "hello");
    }

    #[test]
    fn typing_over_selection_replaces_it() {
        // "hello" 중 "ell" 선택 후 'X' 입력 → "hXo".
        let mut s = st("hello", 1, Some(4));
        s.handle_key(KeyAction::InsertChar('X'));
        assert_eq!(s.input_buffer, "hXo");
        assert_eq!(s.cursor_pos, 2);
        assert_eq!(s.selection_anchor, None);
    }

    #[test]
    fn backspace_with_selection_deletes_selection_only() {
        // 선택이 있으면 Backspace는 선택만 지움 (앞 글자 추가 삭제 안 함).
        let mut s = st("hello", 1, Some(4));
        s.handle_key(KeyAction::Backspace);
        assert_eq!(s.input_buffer, "ho");
        assert_eq!(s.cursor_pos, 1);
    }

    #[test]
    fn ime_commit_over_selection_replaces() {
        // 선택 영역을 한글 commit으로 교체.
        let mut s = st("hello", 1, Some(4));
        s.handle_ime_commit("가");
        assert_eq!(s.input_buffer, "h가o");
        assert_eq!(s.cursor_pos, 1 + "가".len());
        assert_eq!(s.selection_anchor, None);
    }

    #[test]
    fn copy_selection_returns_text_without_modifying() {
        let mut s = st("hello", 1, Some(4));
        assert_eq!(s.copy_selection(), Some("ell".to_string()));
        assert_eq!(s.input_buffer, "hello"); // 변화 없음
        assert_eq!(s.selection_anchor, Some(4)); // 선택 유지 (anchor=4, cursor=1)
    }

    #[test]
    fn cut_selection_returns_text_and_deletes() {
        let mut s = st("hello", 1, Some(4));
        assert_eq!(s.cut_selection(), Some("ell".to_string()));
        assert_eq!(s.input_buffer, "ho");
        assert_eq!(s.cursor_pos, 1);
    }

    #[test]
    fn paste_over_selection_overwrites() {
        // "hello" 중 "ell" 선택 후 "XYZ" paste → "hXYZo".
        let mut s = st("hello", 1, Some(4));
        s.handle_paste("XYZ");
        assert_eq!(s.input_buffer, "hXYZo");
        assert_eq!(s.cursor_pos, 1 + 3);
        assert_eq!(s.selection_anchor, None);
    }

    #[test]
    fn drag_selection_anchor_and_extend() {
        let mut s = st("hello world", 0, None);
        s.start_selection_at(6); // "world" 시작
        assert_eq!(s.cursor_pos, 6);
        assert_eq!(s.selection_anchor, Some(6));
        s.extend_selection_to(11); // 끝까지
        assert_eq!(s.selected_text(), Some("world".to_string()));
    }

    #[test]
    fn submit_clears_selection() {
        let mut s = st("hello", 0, Some(5));
        s.handle_key(KeyAction::Submit);
        assert_eq!(s.selection_anchor, None);
    }
}
