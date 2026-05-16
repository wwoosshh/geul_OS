//! 명령어 파서.
//!
//! 한 줄을 토큰으로 분해한다. 따옴표 처리 지원:
//!   - `"..."` 안의 공백은 보존
//!   - 따옴표 안에서 백슬래시 이스케이프(`\"`, `\\`) 지원

/// 파싱 결과 토큰 목록.
pub type Tokens = Vec<String>;

/// 한 줄을 토큰으로 분해한다.
pub fn tokenize(line: &str) -> Tokens {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escape = false;

    for ch in line.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && in_quote {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            continue;
        }
        if ch.is_whitespace() && !in_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::tokenize;

    #[test]
    fn empty_input() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("    ").is_empty());
    }

    #[test]
    fn simple_split() {
        assert_eq!(tokenize("mount text hello"), vec!["mount", "text", "hello"]);
    }

    #[test]
    fn quoted_string_preserves_spaces() {
        assert_eq!(tokenize(r#"mount text "hello world""#), vec!["mount", "text", "hello world"]);
    }

    #[test]
    fn escaped_quote_inside_quote() {
        assert_eq!(tokenize(r#"echo "say \"hi\"""#), vec!["echo", "say \"hi\""]);
    }

    #[test]
    fn multiple_quoted_args() {
        assert_eq!(
            tokenize(r#"invoke #1 press "force 5""#),
            vec!["invoke", "#1", "press", "force 5"]
        );
    }
}
