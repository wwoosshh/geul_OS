//! 키보드 입력 → 컴포지터 local CLI 상태 변환 (T7.5 v1: ASCII만).
//!
//! 컴포지터는 키 입력을 받아 *서버로 즉시 invoke를 보내지 않는다*. 매 키마다 RPC를 보내면
//! latency가 크기 때문. 입력 버퍼와 커서 위치는 컴포지터 local state로 유지하고,
//! Enter(submit) 시점에만 `submit_input` invoke를 보낸다.
//!
//! T7.5 범위: 영문/숫자/공백 + Backspace + Enter. 한글 IME는 T7.6, 화살표/히스토리는 v2.

/// 컴포지터-사이드 CLI 상태. winit 메인 스레드 안에서만 mutate된다.
#[derive(Debug, Default, Clone)]
pub struct CliLocalState {
    /// 현재 편집 중인 입력 라인 (UTF-8 문자열).
    pub input_buffer: String,
    /// `input_buffer` 안 byte index 커서 위치. ASCII만 다루는 T7.5에서는 char index와 동일.
    pub cursor_pos: usize,
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
}
