//! 두벌식(2-set) 한글 조합 오토마타 — 순수 모듈, I/O 없음.
//!
//! QWERTY 키 → 자모 매핑 + 음절 조합 규칙 (초·중·종성) + 백스페이스 분해.
//! 통합(모드 토글, preedit 표시, CLI commit)은 후속 태스크에서 담당한다.
//!
//! 유니코드 공식: 0xAC00 + (cho * 21 + jung) * 28 + jong
//!
//! 참고 — 초성 19개, 중성 21개, 종성 28개(none=0 포함).

// ─── 초성 테이블 (0..19) ────────────────────────────────────────────────────
// ㄱ ㄲ ㄴ ㄷ ㄸ ㄹ ㅁ ㅂ ㅃ ㅅ ㅆ ㅇ ㅈ ㅉ ㅊ ㅋ ㅌ ㅍ ㅎ
const CHOSEONG: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ',
    'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

// ─── 중성 테이블 (0..21) ────────────────────────────────────────────────────
// ㅏ ㅐ ㅑ ㅒ ㅓ ㅔ ㅕ ㅖ ㅗ ㅘ ㅙ ㅚ ㅛ ㅜ ㅝ ㅞ ㅟ ㅠ ㅡ ㅢ ㅣ
const JUNGSEONG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ',
    'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];

// ─── 종성 테이블 (0..28) ────────────────────────────────────────────────────
// 0=없음, 1=ㄱ … 27=ㅎ
const JONGSEONG: [char; 28] = [
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ',
    'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

// ─── cho 인덱스 조회 ─────────────────────────────────────────────────────────
fn cho_index(c: char) -> Option<usize> {
    CHOSEONG.iter().position(|&x| x == c)
}

// ─── jung 인덱스 조회 ────────────────────────────────────────────────────────
fn jung_index(c: char) -> Option<usize> {
    JUNGSEONG.iter().position(|&x| x == c)
}

// ─── jong 인덱스 조회 (0=없음이므로 실제 jong는 1..) ────────────────────────
fn jong_index(c: char) -> Option<usize> {
    JONGSEONG.iter().position(|&x| x == c)
}

// ─── 복합 중성 조합 ──────────────────────────────────────────────────────────
/// 두 모음을 결합해 복합 중성을 만든다. 정의되지 않은 조합은 None.
fn combine_vowel(base: char, added: char) -> Option<char> {
    match (base, added) {
        ('ㅗ', 'ㅏ') => Some('ㅘ'),
        ('ㅗ', 'ㅐ') => Some('ㅙ'),
        ('ㅗ', 'ㅣ') => Some('ㅚ'),
        ('ㅜ', 'ㅓ') => Some('ㅝ'),
        ('ㅜ', 'ㅔ') => Some('ㅞ'),
        ('ㅜ', 'ㅣ') => Some('ㅟ'),
        ('ㅡ', 'ㅣ') => Some('ㅢ'),
        _ => None,
    }
}

// ─── 복합 종성 조합 ──────────────────────────────────────────────────────────
/// 두 자음을 결합해 복합 종성을 만든다. 정의되지 않은 조합은 None.
fn combine_consonant(base: char, added: char) -> Option<char> {
    match (base, added) {
        ('ㄱ', 'ㅅ') => Some('ㄳ'),
        ('ㄴ', 'ㅈ') => Some('ㄵ'),
        ('ㄴ', 'ㅎ') => Some('ㄶ'),
        ('ㄹ', 'ㄱ') => Some('ㄺ'),
        ('ㄹ', 'ㅁ') => Some('ㄻ'),
        ('ㄹ', 'ㅂ') => Some('ㄼ'),
        ('ㄹ', 'ㅅ') => Some('ㄽ'),
        ('ㄹ', 'ㅌ') => Some('ㄾ'),
        ('ㄹ', 'ㅍ') => Some('ㄿ'),
        ('ㄹ', 'ㅎ') => Some('ㅀ'),
        ('ㅂ', 'ㅅ') => Some('ㅄ'),
        _ => None,
    }
}

/// 복합 종성을 분리한다. 단순 종성은 None, 복합이면 (앞 자음, 뒤 자음) 반환.
fn split_compound_jong(c: char) -> Option<(char, char)> {
    match c {
        'ㄳ' => Some(('ㄱ', 'ㅅ')),
        'ㄵ' => Some(('ㄴ', 'ㅈ')),
        'ㄶ' => Some(('ㄴ', 'ㅎ')),
        'ㄺ' => Some(('ㄹ', 'ㄱ')),
        'ㄻ' => Some(('ㄹ', 'ㅁ')),
        'ㄼ' => Some(('ㄹ', 'ㅂ')),
        'ㄽ' => Some(('ㄹ', 'ㅅ')),
        'ㄾ' => Some(('ㄹ', 'ㅌ')),
        'ㄿ' => Some(('ㄹ', 'ㅍ')),
        'ㅀ' => Some(('ㄹ', 'ㅎ')),
        'ㅄ' => Some(('ㅂ', 'ㅅ')),
        _ => None,
    }
}

// ─── 복합 중성 분리 ──────────────────────────────────────────────────────────
/// 복합 중성을 분리한다. 단순이면 None, 복합이면 (기본 모음, 추가 모음) 반환.
fn split_compound_jung(c: char) -> Option<(char, char)> {
    match c {
        'ㅘ' => Some(('ㅗ', 'ㅏ')),
        'ㅙ' => Some(('ㅗ', 'ㅐ')),
        'ㅚ' => Some(('ㅗ', 'ㅣ')),
        'ㅝ' => Some(('ㅜ', 'ㅓ')),
        'ㅞ' => Some(('ㅜ', 'ㅔ')),
        'ㅟ' => Some(('ㅜ', 'ㅣ')),
        'ㅢ' => Some(('ㅡ', 'ㅣ')),
        _ => None,
    }
}

// ─── 음절 조합 ───────────────────────────────────────────────────────────────
/// 초·중(·종)성 자모에서 유니코드 완성형 한 글자를 만든다.
/// jong == '\0' 또는 종성 없음이면 받침 0.
fn compose_syllable(cho: char, jung: char, jong: char) -> char {
    let ci = cho_index(cho).expect("invalid cho");
    let ji = jung_index(jung).expect("invalid jung");
    let ni = if jong == '\0' { 0 } else { jong_index(jong).expect("invalid jong") };
    let code = 0xAC00u32 + (ci as u32 * 21 + ji as u32) * 28 + ni as u32;
    char::from_u32(code).expect("compose_syllable: bad codepoint")
}

// ─── 자모 분류 ───────────────────────────────────────────────────────────────
fn is_vowel(c: char) -> bool {
    jung_index(c).is_some()
}

#[allow(dead_code)]
fn is_consonant(c: char) -> bool {
    // 단순 자모 + 복합 자모(ㄳ 등)는 cho/jong 테이블 중 하나에 있어야 함.
    // 여기서는 입력 자모는 항상 단순 자모라 가정 (복합은 내부 상태로만 생성됨).
    cho_index(c).is_some() || jong_index(c).is_some()
}

// ─── QWERTY → 자모 매핑 ──────────────────────────────────────────────────────

/// QWERTY 문자(대소문자 및 shift 여부)를 두벌식 자모로 변환한다.
/// 매핑 없으면 None.
pub fn qwerty_to_jamo(c: char) -> Option<char> {
    match c {
        // 언시프트 자음
        'q' => Some('ㅂ'),
        'w' => Some('ㅈ'),
        'e' => Some('ㄷ'),
        'r' => Some('ㄱ'),
        't' => Some('ㅅ'),
        'y' => Some('ㅛ'),
        'u' => Some('ㅕ'),
        'i' => Some('ㅑ'),
        'o' => Some('ㅐ'),
        'p' => Some('ㅔ'),
        'a' => Some('ㅁ'),
        's' => Some('ㄴ'),
        'd' => Some('ㅇ'),
        'f' => Some('ㄹ'),
        'g' => Some('ㅎ'),
        'h' => Some('ㅗ'),
        'j' => Some('ㅓ'),
        'k' => Some('ㅏ'),
        'l' => Some('ㅣ'),
        'z' => Some('ㅋ'),
        'x' => Some('ㅌ'),
        'c' => Some('ㅊ'),
        'v' => Some('ㅍ'),
        'b' => Some('ㅠ'),
        'n' => Some('ㅜ'),
        'm' => Some('ㅡ'),
        // 시프트 전용 (쌍자음, 쌍모음)
        'Q' => Some('ㅃ'),
        'W' => Some('ㅉ'),
        'E' => Some('ㄸ'),
        'R' => Some('ㄲ'),
        'T' => Some('ㅆ'),
        'O' => Some('ㅒ'),
        'P' => Some('ㅖ'),
        // 나머지 시프트 대문자는 소문자와 동일한 자모
        'Y' => Some('ㅛ'),
        'U' => Some('ㅕ'),
        'I' => Some('ㅑ'),
        'A' => Some('ㅁ'),
        'S' => Some('ㄴ'),
        'D' => Some('ㅇ'),
        'F' => Some('ㄹ'),
        'G' => Some('ㅎ'),
        'H' => Some('ㅗ'),
        'J' => Some('ㅓ'),
        'K' => Some('ㅏ'),
        'L' => Some('ㅣ'),
        'Z' => Some('ㅋ'),
        'X' => Some('ㅌ'),
        'C' => Some('ㅊ'),
        'V' => Some('ㅍ'),
        'B' => Some('ㅠ'),
        'N' => Some('ㅜ'),
        'M' => Some('ㅡ'),
        _ => None,
    }
}

// ─── 오토마타 상태 ────────────────────────────────────────────────────────────

/// 조합 중 음절의 내부 상태.
/// cho·jung·jong 슬롯은 Option<char>; jong는 복합 자모('\0' 아님)도 가능.
#[derive(Debug, Clone, Default, PartialEq)]
struct SyllableState {
    cho: Option<char>,
    jung: Option<char>,
    jong: Option<char>,
}

impl SyllableState {
    fn is_empty(&self) -> bool {
        self.cho.is_none() && self.jung.is_none() && self.jong.is_none()
    }

    /// 현재 상태를 한 글자(precomposed 음절 또는 단독 자모)로 렌더링한다.
    fn render(&self) -> Option<char> {
        match (self.cho, self.jung, self.jong) {
            (None, None, None) => None,
            // 초성만: 단독 자모
            (Some(c), None, None) => Some(c),
            // 중성만(초성 없음): 단독 모음
            (None, Some(v), None) => Some(v),
            // 초성+중성 (종성 없음)
            (Some(c), Some(v), None) => Some(compose_syllable(c, v, '\0')),
            // 초성+중성+종성
            (Some(c), Some(v), Some(n)) => Some(compose_syllable(c, v, n)),
            // 비정상 상태 (중성+종성이지만 초성 없음 — 내부적으로 발생 안 함)
            _ => None,
        }
    }
}

// ─── 출력 타입 ───────────────────────────────────────────────────────────────

/// `input_jamo` / `backspace` 처리 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct HangulOutput {
    /// 조합이 확정(commit)된 텍스트. 없으면 빈 문자열.
    pub committed: String,
    /// 현재 조합 중인 음절 (preedit). 없으면 None.
    pub preedit: Option<char>,
}

impl HangulOutput {
    fn empty() -> Self {
        HangulOutput { committed: String::new(), preedit: None }
    }
}

// ─── HangulComposer ──────────────────────────────────────────────────────────

/// 두벌식 한글 조합기.
/// 자모를 하나씩 받아 음절로 조합한다 (pure, no I/O).
#[derive(Debug, Clone, Default)]
pub struct HangulComposer {
    state: SyllableState,
}

impl HangulComposer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 현재 preedit 글자 (조합 중 음절).
    pub fn preedit(&self) -> Option<char> {
        self.state.render()
    }

    /// 현재 조합 중인 음절을 확정(commit)하고 비운다. 없으면 None.
    pub fn flush(&mut self) -> Option<char> {
        let rendered = self.state.render();
        self.state = SyllableState::default();
        rendered
    }

    /// 자모 입력 처리. 음절 경계가 발생하면 committed에 확정분, preedit에 새 조합 중 음절.
    pub fn input_jamo(&mut self, jamo: char) -> HangulOutput {
        if is_vowel(jamo) {
            self.input_vowel(jamo)
        } else {
            self.input_consonant(jamo)
        }
    }

    /// 모음 입력 처리.
    fn input_vowel(&mut self, vowel: char) -> HangulOutput {
        match (self.state.cho, self.state.jung, self.state.jong) {
            // 빈 상태: 모음 단독 → jung 슬롯 (초성 없음)
            (None, None, None) => {
                self.state.jung = Some(vowel);
                HangulOutput { committed: String::new(), preedit: self.state.render() }
            }
            // 초성만: 초성 + 모음 → 음절 시작
            (Some(_), None, None) => {
                self.state.jung = Some(vowel);
                HangulOutput { committed: String::new(), preedit: self.state.render() }
            }
            // 중성만(초성 없음): 복합 모음 시도 또는 commit
            (None, Some(existing_jung), None) => {
                if let Some(combined) = combine_vowel(existing_jung, vowel) {
                    self.state.jung = Some(combined);
                    HangulOutput { committed: String::new(), preedit: self.state.render() }
                } else {
                    // 결합 불가 → 현재 모음 commit, 새 모음 시작
                    let committed = existing_jung.to_string();
                    self.state = SyllableState { jung: Some(vowel), ..Default::default() };
                    HangulOutput { committed, preedit: self.state.render() }
                }
            }
            // 초성+중성 (종성 없음): 복합 모음 시도 또는 commit + 새 시작
            (Some(_cho), Some(existing_jung), None) => {
                if let Some(combined) = combine_vowel(existing_jung, vowel) {
                    self.state.jung = Some(combined);
                    HangulOutput { committed: String::new(), preedit: self.state.render() }
                } else {
                    // 결합 불가 → 현재 음절 commit, 새 모음(초성 없음) 시작
                    let committed = self.state.render().map(|c| c.to_string()).unwrap_or_default();
                    self.state = SyllableState { jung: Some(vowel), ..Default::default() };
                    HangulOutput { committed, preedit: self.state.render() }
                }
            }
            // 초성+중성+종성: 종성 분리 → 앞 글자 commit + 종성이 새 초성으로
            (Some(cho), Some(jung), Some(jong)) => {
                // 복합 종성이면 앞 자음은 그대로 두고 뒤 자음만 분리
                let (committed_jong, new_cho) = if let Some((first, second)) =
                    split_compound_jong(jong)
                {
                    (first, second)
                } else {
                    ('\0', jong) // 단순 종성: 종성 없이 commit
                };

                // commit할 음절 (복합 종성이면 앞 자음 유지, 단순이면 jong 없이)
                let commit_char = if committed_jong == '\0' {
                    compose_syllable(cho, jung, '\0')
                } else {
                    compose_syllable(cho, jung, committed_jong)
                };

                // 새 음절: 분리된 자음 + 새 모음
                self.state = SyllableState {
                    cho: Some(new_cho),
                    jung: Some(vowel),
                    jong: None,
                };
                HangulOutput {
                    committed: commit_char.to_string(),
                    preedit: self.state.render(),
                }
            }
            // 비정상 (중성+종성, 초성 없음) — 방어적으로 flush + 새 모음
            _ => {
                let committed = self.flush().map(|c| c.to_string()).unwrap_or_default();
                self.state.jung = Some(vowel);
                HangulOutput { committed, preedit: self.state.render() }
            }
        }
    }

    /// 자음 입력 처리.
    fn input_consonant(&mut self, consonant: char) -> HangulOutput {
        match (self.state.cho, self.state.jung, self.state.jong) {
            // 빈 상태: 초성 설정
            (None, None, None) => {
                self.state.cho = Some(consonant);
                HangulOutput { committed: String::new(), preedit: self.state.render() }
            }
            // 초성만 (모음 없음): commit 후 새 초성
            (Some(_), None, None) => {
                let committed = self.flush().map(|c| c.to_string()).unwrap_or_default();
                self.state.cho = Some(consonant);
                HangulOutput { committed, preedit: self.state.render() }
            }
            // 모음만 (초성 없음): commit 후 새 초성
            (None, Some(_), None) => {
                let committed = self.flush().map(|c| c.to_string()).unwrap_or_default();
                self.state.cho = Some(consonant);
                HangulOutput { committed, preedit: self.state.render() }
            }
            // 초성+중성: 종성 시도
            (Some(_cho), Some(_jung), None) => {
                if jong_index(consonant).map(|i| i > 0).unwrap_or(false) {
                    // 유효한 종성이면 종성 슬롯으로
                    self.state.jong = Some(consonant);
                    HangulOutput { committed: String::new(), preedit: self.state.render() }
                } else {
                    // 종성 불가 → commit + 새 초성
                    let committed =
                        self.state.render().map(|c| c.to_string()).unwrap_or_default();
                    self.state = SyllableState { cho: Some(consonant), ..Default::default() };
                    HangulOutput { committed, preedit: self.state.render() }
                }
            }
            // 초성+중성+단순 종성: 복합 종성 시도 또는 commit + 새 초성
            (Some(_cho), Some(_jung), Some(existing_jong)) => {
                // 복합 종성 시도
                if let Some(combined) = combine_consonant(existing_jong, consonant) {
                    if jong_index(combined).map(|i| i > 0).unwrap_or(false) {
                        self.state.jong = Some(combined);
                        return HangulOutput {
                            committed: String::new(),
                            preedit: self.state.render(),
                        };
                    }
                }
                // 복합 종성 불가 → 현재 음절 commit + 새 초성
                let committed = self.state.render().map(|c| c.to_string()).unwrap_or_default();
                self.state = SyllableState { cho: Some(consonant), ..Default::default() };
                HangulOutput { committed, preedit: self.state.render() }
            }
            // 비정상
            _ => {
                let committed = self.flush().map(|c| c.to_string()).unwrap_or_default();
                self.state.cho = Some(consonant);
                HangulOutput { committed, preedit: self.state.render() }
            }
        }
    }

    /// 백스페이스: 조합 중 음절을 한 단계 분해한다.
    /// 조합 중 없으면 (빈 output, true) — 호출자가 이미 commit된 글자를 삭제.
    pub fn backspace(&mut self) -> (HangulOutput, bool) {
        if self.state.is_empty() {
            return (HangulOutput::empty(), true);
        }

        match (self.state.cho, self.state.jung, self.state.jong) {
            // 초성만: 비움
            (Some(_), None, None) => {
                self.state.cho = None;
                (HangulOutput { committed: String::new(), preedit: None }, false)
            }
            // 중성만(초성 없음): 비움
            (None, Some(_), None) => {
                self.state.jung = None;
                (HangulOutput { committed: String::new(), preedit: None }, false)
            }
            // 초성+중성 (종성 없음): 복합 모음이면 기본 모음으로, 단순이면 jung 제거
            (Some(_), Some(jung), None) => {
                if let Some((base, _added)) = split_compound_jung(jung) {
                    self.state.jung = Some(base);
                } else {
                    self.state.jung = None;
                }
                (HangulOutput { committed: String::new(), preedit: self.state.render() }, false)
            }
            // 초성+중성+종성: 복합 종성이면 앞 자음으로 축소, 단순이면 jong 제거
            (Some(_), Some(_), Some(jong)) => {
                if let Some((first, _second)) = split_compound_jong(jong) {
                    self.state.jong = Some(first);
                } else {
                    self.state.jong = None;
                }
                (HangulOutput { committed: String::new(), preedit: self.state.render() }, false)
            }
            // 비정상: 비움
            _ => {
                self.state = SyllableState::default();
                (HangulOutput::empty(), false)
            }
        }
    }
}

// ─── 편의 함수 ───────────────────────────────────────────────────────────────

/// 자모 시퀀스를 조합해 완성된 텍스트를 반환한다 (committed + 마지막 preedit 포함).
/// 테스트·디버깅 전용.
pub fn compose_jamo_sequence(jamos: &[char]) -> String {
    let mut composer = HangulComposer::new();
    let mut result = String::new();
    for &jamo in jamos {
        let out = composer.input_jamo(jamo);
        result.push_str(&out.committed);
    }
    if let Some(p) = composer.preedit() {
        result.push(p);
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// 테스트
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── 헬퍼 ──────────────────────────────────────────────────────────────────

    /// 유니코드 공식으로 음절 계산 (테스트 oracle).
    fn syllable(cho: usize, jung: usize, jong: usize) -> char {
        char::from_u32(0xAC00 + (cho as u32 * 21 + jung as u32) * 28 + jong as u32).unwrap()
    }

    // ── qwerty_to_jamo ────────────────────────────────────────────────────────

    #[test]
    fn qwerty_r_maps_to_giyeok() {
        assert_eq!(qwerty_to_jamo('r'), Some('ㄱ'));
    }

    #[test]
    fn qwerty_k_maps_to_a() {
        assert_eq!(qwerty_to_jamo('k'), Some('ㅏ'));
    }

    #[test]
    fn qwerty_shift_r_maps_to_ssanggiyeok() {
        assert_eq!(qwerty_to_jamo('R'), Some('ㄲ'));
    }

    #[test]
    fn qwerty_shift_o_maps_to_yae() {
        assert_eq!(qwerty_to_jamo('O'), Some('ㅒ'));
    }

    #[test]
    fn qwerty_digit_maps_to_none() {
        assert_eq!(qwerty_to_jamo('1'), None);
        assert_eq!(qwerty_to_jamo('0'), None);
        assert_eq!(qwerty_to_jamo(' '), None);
        assert_eq!(qwerty_to_jamo('.'), None);
    }

    #[test]
    fn qwerty_all_unshifted_consonants() {
        let pairs = [
            ('q', 'ㅂ'), ('w', 'ㅈ'), ('e', 'ㄷ'), ('r', 'ㄱ'), ('t', 'ㅅ'),
            ('a', 'ㅁ'), ('s', 'ㄴ'), ('d', 'ㅇ'), ('f', 'ㄹ'), ('g', 'ㅎ'),
            ('z', 'ㅋ'), ('x', 'ㅌ'), ('c', 'ㅊ'), ('v', 'ㅍ'),
        ];
        for (key, jamo) in pairs {
            assert_eq!(qwerty_to_jamo(key), Some(jamo), "key={key}");
        }
    }

    #[test]
    fn qwerty_all_unshifted_vowels() {
        let pairs = [
            ('y', 'ㅛ'), ('u', 'ㅕ'), ('i', 'ㅑ'), ('o', 'ㅐ'), ('p', 'ㅔ'),
            ('h', 'ㅗ'), ('j', 'ㅓ'), ('k', 'ㅏ'), ('l', 'ㅣ'),
            ('b', 'ㅠ'), ('n', 'ㅜ'), ('m', 'ㅡ'),
        ];
        for (key, jamo) in pairs {
            assert_eq!(qwerty_to_jamo(key), Some(jamo), "key={key}");
        }
    }

    #[test]
    fn qwerty_shifted_doubles() {
        assert_eq!(qwerty_to_jamo('Q'), Some('ㅃ'));
        assert_eq!(qwerty_to_jamo('W'), Some('ㅉ'));
        assert_eq!(qwerty_to_jamo('E'), Some('ㄸ'));
        assert_eq!(qwerty_to_jamo('T'), Some('ㅆ'));
        assert_eq!(qwerty_to_jamo('P'), Some('ㅖ'));
    }

    // ── 단순 음절 ─────────────────────────────────────────────────────────────

    #[test]
    fn simple_syllable_ga() {
        // ㄱ + ㅏ → 가
        let mut c = HangulComposer::new();
        let o1 = c.input_jamo('ㄱ');
        assert_eq!(o1.committed, "");
        assert_eq!(o1.preedit, Some('ㄱ'));

        let o2 = c.input_jamo('ㅏ');
        assert_eq!(o2.committed, "");
        // 가 = 0xAC00 + (0*21+0)*28+0 = 0xAC00
        assert_eq!(o2.preedit, Some(syllable(0, 0, 0))); // 가
    }

    #[test]
    fn syllable_with_jong_gan() {
        // ㄱ + ㅏ + ㄴ → 간
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        let o = c.input_jamo('ㄴ');
        assert_eq!(o.committed, "");
        // 간: cho=ㄱ(0), jung=ㅏ(0), jong=ㄴ(4)
        assert_eq!(o.preedit, Some(syllable(0, 0, 4)));
    }

    // ── 종성 분리 ─────────────────────────────────────────────────────────────

    #[test]
    fn jong_deattach_gan_a_to_ga_na() {
        // ㄱ ㅏ ㄴ ㅏ → committed="가", preedit="나"
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        c.input_jamo('ㄴ');
        let o = c.input_jamo('ㅏ');
        assert_eq!(o.committed, "가"); // 가 = syllable(0,0,0)
        // 나: cho=ㄴ(2), jung=ㅏ(0), jong=0
        assert_eq!(o.preedit, Some(syllable(2, 0, 0))); // 나
    }

    // ── 복합 모음 ─────────────────────────────────────────────────────────────

    #[test]
    fn compound_vowel_hwa() {
        // ㅎ + ㅗ + ㅏ → 화 (화)
        let mut c = HangulComposer::new();
        c.input_jamo('ㅎ');
        c.input_jamo('ㅗ');
        let o = c.input_jamo('ㅏ');
        // 화: cho=ㅎ(18), jung=ㅘ(9), jong=0
        assert_eq!(o.committed, "");
        assert_eq!(o.preedit, Some(syllable(18, 9, 0))); // 화
    }

    // ── 복합 종성 ─────────────────────────────────────────────────────────────

    #[test]
    fn compound_jong_dak() {
        // ㄷ ㅏ ㄹ ㄱ → 닭
        let mut c = HangulComposer::new();
        c.input_jamo('ㄷ');
        c.input_jamo('ㅏ');
        c.input_jamo('ㄹ');
        let o = c.input_jamo('ㄱ');
        assert_eq!(o.committed, "");
        // 닭: cho=ㄷ(3), jung=ㅏ(0), jong=ㄺ(9)
        assert_eq!(o.preedit, Some(syllable(3, 0, 9))); // 닭
    }

    #[test]
    fn compound_jong_deattach_dak_a() {
        // 닭 + ㅏ → committed="달", preedit="가"
        // ㄷ ㅏ ㄹ ㄱ ㅏ
        let mut c = HangulComposer::new();
        c.input_jamo('ㄷ');
        c.input_jamo('ㅏ');
        c.input_jamo('ㄹ');
        c.input_jamo('ㄱ');
        let o = c.input_jamo('ㅏ');
        // 달: cho=ㄷ(3), jung=ㅏ(0), jong=ㄹ(8)
        let dal = syllable(3, 0, 8);
        // 가: cho=ㄱ(0), jung=ㅏ(0), jong=0
        let ga = syllable(0, 0, 0);
        assert_eq!(o.committed, dal.to_string(), "committed should be 달");
        assert_eq!(o.preedit, Some(ga), "preedit should be 가");
    }

    // ── 쌍자음 ───────────────────────────────────────────────────────────────

    #[test]
    fn double_consonant_kka() {
        // ㄲ(R shift) + ㅏ → 까
        let mut c = HangulComposer::new();
        c.input_jamo('ㄲ');
        let o = c.input_jamo('ㅏ');
        // 까: cho=ㄲ(1), jung=ㅏ(0), jong=0
        assert_eq!(o.committed, "");
        assert_eq!(o.preedit, Some(syllable(1, 0, 0))); // 까
    }

    // ── "안녕" 완성 ──────────────────────────────────────────────────────────

    #[test]
    fn full_word_annyeong() {
        // 자모 시퀀스: ㅇ ㅏ ㄴ ㄴ ㅕ ㅇ → "안녕"
        // 단계:
        //   ㅇ → cho=ㅇ
        //   ㅏ → 아
        //   ㄴ → 안
        //   ㄴ → commit 안, new cho=ㄴ
        //   ㅕ → 녀
        //   ㅇ → 녕
        // 최종: committed 누적 = "안", preedit = "녕"
        let mut c = HangulComposer::new();
        let mut acc = String::new();

        let o = c.input_jamo('ㅇ');
        acc.push_str(&o.committed);
        let o = c.input_jamo('ㅏ');
        acc.push_str(&o.committed);
        let o = c.input_jamo('ㄴ');
        acc.push_str(&o.committed);
        let o = c.input_jamo('ㄴ');
        acc.push_str(&o.committed); // "안" committed here
        let o = c.input_jamo('ㅕ');
        acc.push_str(&o.committed);
        let o = c.input_jamo('ㅇ');
        acc.push_str(&o.committed);

        // preedit = "녕"
        let preedit = c.preedit();
        let visible: String = acc + &preedit.map(|c| c.to_string()).unwrap_or_default();

        // 안: cho=ㅇ(11), jung=ㅏ(0), jong=ㄴ(4)
        let an = syllable(11, 0, 4);
        // 녕: cho=ㄴ(2), jung=ㅕ(6), jong=ㅇ(21)
        let nyeong = syllable(2, 6, 21);
        assert_eq!(visible, format!("{an}{nyeong}"), "visible text should be 안녕");
    }

    // ── 백스페이스 분해 ──────────────────────────────────────────────────────

    #[test]
    fn backspace_decomposes_jong() {
        // 간 → 가 (ㄴ 제거)
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        c.input_jamo('ㄴ');
        let (out, del) = c.backspace();
        assert!(!del, "should not delete committed");
        assert_eq!(out.preedit, Some(syllable(0, 0, 0))); // 가
    }

    #[test]
    fn backspace_decomposes_jung() {
        // 가 → ㄱ (ㅏ 제거)
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        let (out, del) = c.backspace();
        assert!(!del);
        assert_eq!(out.preedit, Some('ㄱ'));
    }

    #[test]
    fn backspace_decomposes_cho() {
        // ㄱ → empty
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        let (out, del) = c.backspace();
        assert!(!del);
        assert_eq!(out.preedit, None);
    }

    #[test]
    fn backspace_on_empty_signals_delete_committed() {
        let mut c = HangulComposer::new();
        let (out, del) = c.backspace();
        assert!(del, "empty state → delete committed");
        assert_eq!(out.committed, "");
        assert_eq!(out.preedit, None);
    }

    #[test]
    fn backspace_compound_jong_decomposes_to_first() {
        // 닭(ㄷㅏㄺ) → 달(ㄷㅏㄹ)
        let mut c = HangulComposer::new();
        c.input_jamo('ㄷ');
        c.input_jamo('ㅏ');
        c.input_jamo('ㄹ');
        c.input_jamo('ㄱ'); // 닭
        let (out, del) = c.backspace();
        assert!(!del);
        // 달: cho=ㄷ(3), jung=ㅏ(0), jong=ㄹ(8)
        assert_eq!(out.preedit, Some(syllable(3, 0, 8)));
    }

    #[test]
    fn backspace_compound_jung_decomposes_to_base() {
        // 화(ㅎㅘ) backspace → ㅎ + ㅗ (ㅏ 제거 → ㅘ → ㅗ)
        let mut c = HangulComposer::new();
        c.input_jamo('ㅎ');
        c.input_jamo('ㅗ');
        c.input_jamo('ㅏ'); // → 화
        let (out, del) = c.backspace(); // 화 → 호
        assert!(!del);
        // 호: cho=ㅎ(18), jung=ㅗ(8), jong=0
        assert_eq!(out.preedit, Some(syllable(18, 8, 0)));
    }

    // ── flush ─────────────────────────────────────────────────────────────────

    #[test]
    fn flush_commits_and_clears() {
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        let committed = c.flush();
        assert_eq!(committed, Some(syllable(0, 0, 0))); // 가
        assert_eq!(c.preedit(), None);
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut c = HangulComposer::new();
        assert_eq!(c.flush(), None);
    }

    // ── compose_jamo_sequence helper ─────────────────────────────────────────

    #[test]
    fn compose_sequence_annyeong() {
        let jamos = ['ㅇ', 'ㅏ', 'ㄴ', 'ㄴ', 'ㅕ', 'ㅇ'];
        assert_eq!(compose_jamo_sequence(&jamos), "안녕");
    }

    #[test]
    fn compose_sequence_hwa() {
        let jamos = ['ㅎ', 'ㅗ', 'ㅏ'];
        assert_eq!(compose_jamo_sequence(&jamos), "화");
    }

    #[test]
    fn compose_sequence_dak() {
        let jamos = ['ㄷ', 'ㅏ', 'ㄹ', 'ㄱ'];
        assert_eq!(compose_jamo_sequence(&jamos), "닭");
    }

    // ── 단독 모음/자음 ────────────────────────────────────────────────────────

    #[test]
    fn lone_vowel_no_cho() {
        // ㅏ 만 입력 → preedit = 'ㅏ' (단독 모음)
        let mut c = HangulComposer::new();
        let o = c.input_jamo('ㅏ');
        assert_eq!(o.preedit, Some('ㅏ'));
        assert_eq!(o.committed, "");
    }

    #[test]
    fn two_consonants_no_vowel() {
        // ㄱ ㄴ → commit ㄱ + preedit ㄴ
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        let o = c.input_jamo('ㄴ');
        assert_eq!(o.committed, "ㄱ");
        assert_eq!(o.preedit, Some('ㄴ'));
    }

    // ── 추가 케이스 ───────────────────────────────────────────────────────────

    #[test]
    fn compound_vowel_ui() {
        // ㅡ + ㅣ → ㅢ
        // ㄱ ㅡ ㅣ → 긔
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅡ');
        let o = c.input_jamo('ㅣ');
        assert_eq!(o.committed, "");
        // 긔: cho=ㄱ(0), jung=ㅢ(19), jong=0
        assert_eq!(o.preedit, Some(syllable(0, 19, 0)));
    }

    #[test]
    fn non_compound_vowels_trigger_commit() {
        // ㄱ ㅏ ㅏ → committed="가", new preedit="ㅏ"
        let mut c = HangulComposer::new();
        c.input_jamo('ㄱ');
        c.input_jamo('ㅏ');
        let o = c.input_jamo('ㅏ');
        assert_eq!(o.committed, "가");
        assert_eq!(o.preedit, Some('ㅏ'));
    }

    #[test]
    fn full_sequence_via_qwerty() {
        // QWERTY 키로 "안녕" 타이핑: d k s s j d
        // d→ㅇ, k→ㅏ, s→ㄴ, s→ㄴ, j→ㅓ... wait
        // 안녕 = ㅇㅏㄴ / ㄴㅕㅇ
        // d=ㅇ, k=ㅏ, s=ㄴ, s=ㄴ, u=ㅕ, d=ㅇ
        let keys = ['d', 'k', 's', 's', 'u', 'd'];
        let jamos: Vec<char> = keys.iter().filter_map(|&k| qwerty_to_jamo(k)).collect();
        assert_eq!(compose_jamo_sequence(&jamos), "안녕");
    }

    #[test]
    fn full_sequence_dak_via_qwerty() {
        // 닭 = ㄷㅏㄹㄱ: e k f r
        let keys = ['e', 'k', 'f', 'r'];
        let jamos: Vec<char> = keys.iter().filter_map(|&k| qwerty_to_jamo(k)).collect();
        assert_eq!(compose_jamo_sequence(&jamos), "닭");
    }

    #[test]
    fn full_sequence_hwa_via_qwerty() {
        // 화 = ㅎㅗㅏ: g h k
        let keys = ['g', 'h', 'k'];
        let jamos: Vec<char> = keys.iter().filter_map(|&k| qwerty_to_jamo(k)).collect();
        assert_eq!(compose_jamo_sequence(&jamos), "화");
    }
}
