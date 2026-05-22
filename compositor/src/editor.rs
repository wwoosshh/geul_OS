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
        // cursor 초기 = 0 (맨 앞). 메모장 등 일반 에디터 통념. v1은 마우스 클릭으로 cursor
        // 위치를 산출하기 전이라 0이 가장 안전한 기본값 — 끝(content.len())에 두면 사용자가
        // 큰 파일을 열었을 때 *맨 아래*에 cursor가 박혀 보임.
        Self { window_id, content, cursor: 0 }
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

    /// 마우스 클릭으로 cursor를 *visual* (line, col)에 가깝게 이동.
    ///
    /// content가 wrap된 *시각* line/col 좌표를 받아 *byte offset*으로 변환해 cursor 갱신.
    /// `chars_per_line`은 render의 wrap 폭과 같은 값(예: 14 char). target이 content 범위를
    /// 넘으면 가장 가까운 char boundary로 clamp (line 끝 또는 content 끝).
    pub fn set_cursor_from_visual(&mut self, t_line: i32, t_col: i32, chars_per_line: usize) {
        self.cursor = visual_to_byte_offset(&self.content, t_line, t_col, chars_per_line);
    }
}

/// `content`의 시작부터 char 단위로 순회하며 (visual line, col) 좌표에 해당하는 *byte offset*
/// 반환. wrap은 `chars_per_line` 도달 시 자동 줄바꿈으로 처리, `\n`은 line 강제 종료.
///
/// target에 도달하면 그 시점 byte offset 반환. target line의 *행 끝*(다음 \n 직전 또는 wrap
/// 직전)을 넘어선 col이면 행 끝 offset. target line 자체를 넘어선 line이면 content 끝.
pub fn visual_to_byte_offset(
    content: &str,
    t_line: i32,
    t_col: i32,
    chars_per_line: usize,
) -> usize {
    let mut line = 0i32;
    let mut col = 0i32;
    let mut byte_offset = 0usize;
    for c in content.chars() {
        if line == t_line && col == t_col {
            return byte_offset;
        }
        if line > t_line {
            return byte_offset;
        }
        let c_len = c.len_utf8();
        if c == '\n' {
            if line == t_line {
                return byte_offset;
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
            if (col as usize) >= chars_per_line {
                if line == t_line {
                    return byte_offset + c_len;
                }
                line += 1;
                col = 0;
            }
        }
        byte_offset += c_len;
    }
    byte_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(s: &str) -> EditorState {
        EditorState::new(ObjectId::new(), s.to_string())
    }

    #[test]
    fn new_cursor_at_start() {
        // cursor 초기 0 — 메모장처럼 *맨 앞*. 사용자가 큰 파일을 열어도 끝에 박히지 않음.
        let e = ed("hello");
        assert_eq!(e.cursor, 0);
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
        e.cursor = 2;
        e.backspace();
        assert_eq!(e.content, "a");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn backspace_removes_korean_char_three_bytes() {
        let mut e = ed("한글");
        e.cursor = 6;
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
    fn visual_to_byte_first_line_first_col() {
        assert_eq!(visual_to_byte_offset("hello\nworld", 0, 0, 80), 0);
    }

    #[test]
    fn visual_to_byte_first_line_mid_col() {
        assert_eq!(visual_to_byte_offset("hello\nworld", 0, 3, 80), 3);
    }

    #[test]
    fn visual_to_byte_second_line_first_col() {
        // "hello\n" 6 bytes 다음 'w'의 위치.
        assert_eq!(visual_to_byte_offset("hello\nworld", 1, 0, 80), 6);
    }

    #[test]
    fn visual_to_byte_korean_advances_three_bytes() {
        // "한글" 두 char. (line=0, col=1)이면 '한' 다음 = byte 3.
        assert_eq!(visual_to_byte_offset("한글", 0, 1, 80), 3);
    }

    #[test]
    fn visual_to_byte_past_line_end_clamps_to_lf() {
        // "ab\ncd" — (0, 10) 요청 → '\n' 직전 = byte 2.
        assert_eq!(visual_to_byte_offset("ab\ncd", 0, 10, 80), 2);
    }

    #[test]
    fn visual_to_byte_past_content_clamps_to_end() {
        assert_eq!(visual_to_byte_offset("ab", 99, 0, 80), 2);
    }

    #[test]
    fn set_cursor_from_visual_korean() {
        let mut e = ed("한\n글");
        e.set_cursor_from_visual(1, 1, 80);
        // "한\n글" — line 1, col 1 = '글' 다음 = byte 3 + 1(\n) + 3 = 7.
        assert_eq!(e.cursor, 7);
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
