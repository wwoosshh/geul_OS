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
}

impl CliLocalState {
    /// 키 입력 한 건을 처리한다. Enter면 Some(submitted_text)를 반환하고 버퍼는 비운다.
    /// 그 외엔 None — 호출자가 redraw만 요청.
    pub fn handle_key(&mut self, action: KeyAction) -> Option<String> {
        match action {
            KeyAction::InsertChar(c) => {
                // ASCII 가시 문자 + 공백만 허용. 제어문자는 무시 (Tab 등은 v2).
                if c == ' ' || (c.is_ascii_graphic() && !c.is_control()) {
                    let mut tmp = String::with_capacity(4);
                    tmp.push(c);
                    self.input_buffer.insert_str(self.cursor_pos, &tmp);
                    self.cursor_pos += tmp.len();
                }
                None
            }
            KeyAction::Backspace => {
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
        self.input_buffer.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.preedit_text.clear();
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
}
