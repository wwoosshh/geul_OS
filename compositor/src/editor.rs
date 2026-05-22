//! Window edit_mode 시 사용되는 컴포지터-사이드 editor state (M9 / ADR-035).
//!
//! cursor는 *byte offset* (UTF-8 char boundary 위에 항상 있도록 char insert/delete가 보장).
//! content는 server-state Window.content의 *컴포지터 측 미러* — 키 입력마다 즉시 미러 갱신 +
//! debounced로 server에 SetState. v1은 *모든 변경*을 즉시 SetState (debounce는 v2).

use geulos_core::ObjectId;

/// 한 Window의 편집 상태. compositor가 KeyboardFocus::Window(id) 시 active로 유지.
#[derive(Debug, Clone)]
pub struct EditorState {
    pub window_id: ObjectId,
    pub content: String,
    /// byte offset (항상 char boundary).
    pub cursor: usize,
}

impl EditorState {
    pub fn new(window_id: ObjectId, content: String) -> Self {
        let cursor = content.len();
        Self { window_id, content, cursor }
    }

    /// 한 char 삽입 — cursor 위치에. cursor를 char width(byte 수)만큼 전진.
    pub fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Backspace — cursor 바로 앞의 한 char 삭제. cursor가 0이면 무동작.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev =
            self.content[..self.cursor].chars().next_back().expect("cursor > 0 → prev char 존재");
        let prev_byte_len = prev.len_utf8();
        self.cursor -= prev_byte_len;
        self.content.drain(self.cursor..self.cursor + prev_byte_len);
    }

    /// cursor 왼쪽으로 한 char.
    pub fn cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.content[..self.cursor].chars().next_back().expect("cursor > 0 → prev 존재");
        self.cursor -= prev.len_utf8();
    }

    /// cursor 오른쪽으로 한 char.
    pub fn cursor_right(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let next = self.content[self.cursor..].chars().next().expect("cursor < len → next 존재");
        self.cursor += next.len_utf8();
    }

    /// 엔터 — '\n' 삽입.
    pub fn newline(&mut self) {
        self.insert_char('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(s: &str) -> EditorState {
        EditorState::new(ObjectId::new(), s.to_string())
    }

    #[test]
    fn new_cursor_at_end() {
        let e = ed("hello");
        assert_eq!(e.cursor, 5);
    }

    #[test]
    fn insert_ascii_advances_one() {
        let mut e = ed("");
        e.insert_char('a');
        assert_eq!(e.content, "a");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn insert_korean_advances_three_bytes() {
        let mut e = ed("");
        e.insert_char('한');
        assert_eq!(e.content, "한");
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn backspace_removes_prev_char() {
        let mut e = ed("ab");
        e.backspace();
        assert_eq!(e.content, "a");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn backspace_removes_korean_char_three_bytes() {
        let mut e = ed("한글");
        e.backspace();
        assert_eq!(e.content, "한");
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn backspace_at_zero_no_op() {
        let mut e = ed("");
        e.backspace();
        assert_eq!(e.content, "");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn cursor_left_right_respect_korean_boundary() {
        let mut e = ed("a한b");
        e.cursor = 0;
        e.cursor_right();
        assert_eq!(e.cursor, 1);
        e.cursor_right();
        assert_eq!(e.cursor, 4);
        e.cursor_right();
        assert_eq!(e.cursor, 5);
        e.cursor_right();
        assert_eq!(e.cursor, 5);
        e.cursor_left();
        assert_eq!(e.cursor, 4);
        e.cursor_left();
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn newline_inserts_lf() {
        let mut e = ed("ab");
        e.cursor = 1;
        e.newline();
        assert_eq!(e.content, "a\nb");
        assert_eq!(e.cursor, 2);
    }
}
