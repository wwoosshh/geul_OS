//! Window 편집을 위한 컴포지터-사이드 editor state (M9 / ADR-035).
//!
//! cursor는 *byte offset* (UTF-8 char boundary 위에 항상 있도록 char insert/delete가 보장).
//! content는 컴포지터 *local-master* — 키 입력 시 즉시 local 갱신, save 시점에만 invoke
//! args로 디스크 commit. server는 dirty boolean만 SetState.

use geulos_core::ObjectId;

/// 한 Window의 편집 상태. compositor가 KeyboardFocus::Window(id) 시 active로 유지.
#[derive(Debug, Clone)]
pub struct EditorState {
    pub window_id: ObjectId,
    pub content: String,
    /// byte offset (항상 char boundary).
    pub cursor: usize,
    /// server에 dirty=true SetState를 이미 보냈는지. 매 키 입력마다 SetState를 보내면
    /// mpsc/wire backpressure로 입력 freeze (사용자 보고). 한 번만 보내고 save_to_file
    /// 성공 후 reset.
    pub dirty_synced: bool,
}

impl EditorState {
    pub fn new(window_id: ObjectId, content: String) -> Self {
        // cursor 초기 = 0 (맨 앞). 메모장 등 일반 에디터 통념. 큰 파일 열어도 맨 아래에
        // 박히지 않음.
        Self { window_id, content, cursor: 0, dirty_synced: false }
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

/// 한 wrap된 시각 line — 원본 content 안의 시작 byte offset + 그 line text.
///
/// `\n`은 line text에 포함되지 않으며 *line 경계*만 결정. wrap (max_w_px 초과)도 line 경계.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapLine {
    pub start_byte: usize,
    pub text: String,
}

/// content를 *fontdue measure 기반*으로 wrap. 시각 line 목록 반환.
///
/// `\n`은 line 강제 종료. 한 char 추가 시 그 line의 *실제 측정 폭*이 `max_w_px` 초과하면
/// 그 char를 다음 line으로 밀어 줄바꿈. advance만 누적하던 v1은 glyph bbox와 advance 차이로
/// 시각이 wrap_w를 1-2px 넘는 경우가 있었음 (사용자 보고: "텍스트가 창을 벗어남"). measure는
/// fontdue가 그리는 *정확한 visual right*이므로 어긋남 없음.
///
/// 복잡도: line당 char 수 N일 때 O(N²) (각 추가마다 layout). 7KB 텍스트 (~7K char) + line당
/// 평균 100 char라면 ~700K ops — 매 render 호출 시도 1ms 안. 핫패스가 되면 캐싱 검토 (v2).
pub fn wrap_by_pixel_width(content: &str, max_w_px: i32) -> Vec<WrapLine> {
    let mut out: Vec<WrapLine> = Vec::new();
    let mut current_text = String::new();
    let mut current_start: usize = 0;
    let mut byte_pos: usize = 0;
    for c in content.chars() {
        let c_len = c.len_utf8();
        if c == '\n' {
            out.push(WrapLine {
                start_byte: current_start,
                text: std::mem::take(&mut current_text),
            });
            byte_pos += c_len;
            current_start = byte_pos;
            continue;
        }
        if max_w_px > 0 && !current_text.is_empty() {
            // 한 char를 *시험 추가*해서 실제 측정 폭이 한도 초과인지 확인.
            current_text.push(c);
            let actual_w = crate::text::measure_text_width(&current_text);
            if actual_w > max_w_px {
                // 초과 — 그 char를 빼고 line을 push, 그 char를 다음 line의 첫 글자로.
                current_text.pop();
                out.push(WrapLine {
                    start_byte: current_start,
                    text: std::mem::take(&mut current_text),
                });
                current_start = byte_pos;
                current_text.push(c);
            }
        } else {
            current_text.push(c);
        }
        byte_pos += c_len;
    }
    out.push(WrapLine { start_byte: current_start, text: current_text });
    out
}

/// 마우스 클릭 좌표(visual line + 해당 line 내 pixel x)를 *byte offset*으로 변환.
///
/// `lines`는 `wrap_by_pixel_width` 결과 — render와 동일 wrap을 사용해야 cursor 시각화와
/// click hit가 일치. `click_line_idx`가 lines.len() 초과면 content 끝. line 내에서 각 char의
/// advance를 누적하다가 `click_x_px`에 *그 char 중앙*까지 못 미치면 그 char *직전* byte를
/// 선택 (자연스러운 메모장 UX).
pub fn byte_offset_from_pixel(lines: &[WrapLine], click_line_idx: usize, click_x_px: i32) -> usize {
    if lines.is_empty() {
        return 0;
    }
    if click_line_idx >= lines.len() {
        let last = &lines[lines.len() - 1];
        return last.start_byte + last.text.len();
    }
    let line = &lines[click_line_idx];
    let target = click_x_px;
    // *fontdue measure 기반* — render의 wrap과 동일 측정으로 cursor 위치가 텍스트와 정확
    // 일치. 각 char 추가 후 prefix measure를 누적해 char 중앙 (prev_w + w)/2과 비교.
    let mut prefix = String::new();
    let mut byte_in_line: usize = 0;
    let mut prev_w = 0i32;
    for c in line.text.chars() {
        prefix.push(c);
        let w = crate::text::measure_text_width(&prefix);
        let mid = (prev_w + w) / 2;
        if target < mid {
            break;
        }
        byte_in_line += c.len_utf8();
        prev_w = w;
    }
    line.start_byte + byte_in_line
}

/// cursor가 어느 visual line의 어디(byte offset within line)에 있는지 산출.
///
/// `lines`는 `wrap_by_pixel_width` 결과. cursor가 line의 [start_byte, start_byte+text.len()]
/// 범위 안이면 그 line. cursor가 line 끝(다음 line 시작 직전 = `\n` 위치)이면 *이전 line의 끝*
/// 으로 본다 (메모장 cursor가 줄 끝에 보이도록).
pub fn line_and_byte_in_line(lines: &[WrapLine], cursor: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    for (i, line) in lines.iter().enumerate() {
        let line_end = line.start_byte + line.text.len();
        if cursor >= line.start_byte && cursor <= line_end {
            return (i, cursor - line.start_byte);
        }
    }
    // cursor가 content 끝을 넘으면 마지막 line의 끝.
    let last_idx = lines.len() - 1;
    let last = &lines[last_idx];
    (last_idx, last.text.len())
}

/// `content`의 시작부터 char 단위로 순회하며 (visual line, col) 좌표에 해당하는 *byte offset*
/// 반환. wrap은 `chars_per_line` 도달 시 자동 줄바꿈으로 처리, `\n`은 line 강제 종료.
///
/// **deprecated** — wrap_by_pixel_width + byte_offset_from_pixel 조합으로 대체. char width
/// 14 휴리스틱이 한글에서 어긋났음. 단위 테스트 호환을 위해 유지.
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
    fn wrap_empty_content_yields_one_line() {
        let lines = wrap_by_pixel_width("", 200);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].start_byte, 0);
        assert_eq!(lines[0].text, "");
    }

    #[test]
    fn wrap_no_newline_short_text_one_line() {
        let lines = wrap_by_pixel_width("hello", 10_000);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "hello");
        assert_eq!(lines[0].start_byte, 0);
    }

    #[test]
    fn wrap_newline_splits() {
        let lines = wrap_by_pixel_width("ab\ncd\nef", 10_000);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "ab");
        assert_eq!(lines[0].start_byte, 0);
        assert_eq!(lines[1].text, "cd");
        assert_eq!(lines[1].start_byte, 3); // "ab\n"
        assert_eq!(lines[2].text, "ef");
        assert_eq!(lines[2].start_byte, 6);
    }

    #[test]
    fn line_and_byte_in_line_basic() {
        let lines = wrap_by_pixel_width("ab\ncd", 10_000);
        // cursor at byte 0 → line 0, in_line 0
        assert_eq!(line_and_byte_in_line(&lines, 0), (0, 0));
        // cursor at byte 2 → line 0, in_line 2 (line end)
        assert_eq!(line_and_byte_in_line(&lines, 2), (0, 2));
        // cursor at byte 3 → line 1, in_line 0 (\n 다음)
        assert_eq!(line_and_byte_in_line(&lines, 3), (1, 0));
        // cursor at end → line 1, in_line 2
        assert_eq!(line_and_byte_in_line(&lines, 5), (1, 2));
    }

    #[test]
    fn byte_offset_from_pixel_click_at_zero_returns_line_start() {
        let lines = wrap_by_pixel_width("hello", 10_000);
        assert_eq!(byte_offset_from_pixel(&lines, 0, 0), 0);
    }

    #[test]
    fn byte_offset_from_pixel_click_past_end_returns_line_end() {
        let lines = wrap_by_pixel_width("hi", 10_000);
        assert_eq!(byte_offset_from_pixel(&lines, 0, 10_000), 2);
    }

    #[test]
    fn byte_offset_from_pixel_past_line_index_clamps_to_end() {
        let lines = wrap_by_pixel_width("abc", 10_000);
        assert_eq!(byte_offset_from_pixel(&lines, 99, 0), 3);
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
