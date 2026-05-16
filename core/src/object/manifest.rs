//! 앱 매니페스트 (`aios.toml`).
//!
//! 앱이 시작할 때 자기 정체·요구 권한·사용 ui_types를 선언한다.
//! 서버는 Hello 시 검증해 ActorId(`app:<id>:<uuid>`)를 발급.

use thiserror::Error;

use super::identity::TypeUri;

/// 앱 매니페스트.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    /// 앱 고유 ID (영문/숫자/`-`/`_`).
    pub id: String,
    /// 카테고리 권한 목록 (예: `fs.user.docs`).
    pub permissions: Vec<String>,
    /// 이 앱이 사용할 객체 타입 URI 목록.
    pub ui_types: Vec<TypeUri>,
}

/// 매니페스트 파싱 오류.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// TOML 파싱 실패.
    #[error("TOML parse error: {0}")]
    Toml(String),
    /// TypeUri 파싱 실패.
    #[error("bad TypeUri: {0}")]
    BadTypeUri(String),
}

/// 간단한 TOML 값 타입 (매니페스트에 필요한 것만).
enum TomlValue {
    Str(String),
    Array(Vec<String>),
}

/// 최소한의 TOML 파서 — 매니페스트 형식만 지원.
///
/// 지원 형식:
/// - `key = "value"`
/// - `key = []`
/// - `key = ["a", "b", "c"]`
fn parse_minimal_toml(s: &str) -> Result<std::collections::HashMap<String, TomlValue>, String> {
    let mut map = std::collections::HashMap::new();

    let mut pos = 0usize;

    // 전체 입력을 처리
    let bytes = s.as_bytes();
    let len = bytes.len();

    while pos < len {
        // 공백/줄 건너뜀
        while pos < len
            && (bytes[pos] == b' '
                || bytes[pos] == b'\t'
                || bytes[pos] == b'\r'
                || bytes[pos] == b'\n')
        {
            pos += 1;
        }
        if pos >= len {
            break;
        }
        // 주석 처리
        if bytes[pos] == b'#' {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        // 키 파싱 (영문자, 숫자, _, -)
        let key_start = pos;
        while pos < len
            && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_' || bytes[pos] == b'-')
        {
            pos += 1;
        }
        if pos == key_start {
            return Err(format!(
                "unexpected character at position {}: {:?}",
                pos, bytes[pos] as char
            ));
        }
        let key = &s[key_start..pos];

        // 공백 건너뜀
        while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        // '=' 기대
        if pos >= len || bytes[pos] != b'=' {
            return Err(format!("expected '=' after key '{}' at position {}", key, pos));
        }
        pos += 1; // skip '='
                  // 공백 건너뜀
        while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if pos >= len {
            return Err(format!("unexpected end after '=' for key '{}'", key));
        }

        if bytes[pos] == b'"' {
            // 문자열 파싱
            pos += 1; // skip opening '"'
            let val_start = pos;
            while pos < len && bytes[pos] != b'"' {
                pos += 1;
            }
            if pos >= len {
                return Err(format!("unterminated string for key '{}'", key));
            }
            let val = s[val_start..pos].to_string();
            pos += 1; // skip closing '"'
            map.insert(key.to_string(), TomlValue::Str(val));
        } else if bytes[pos] == b'[' {
            // 배열 파싱
            pos += 1; // skip '['
            let mut items = Vec::new();
            loop {
                // 공백 건너뜀
                while pos < len
                    && (bytes[pos] == b' '
                        || bytes[pos] == b'\t'
                        || bytes[pos] == b'\r'
                        || bytes[pos] == b'\n')
                {
                    pos += 1;
                }
                if pos >= len {
                    return Err(format!("unterminated array for key '{}'", key));
                }
                if bytes[pos] == b']' {
                    pos += 1; // skip ']'
                    break;
                }
                if bytes[pos] == b'"' {
                    pos += 1; // skip opening '"'
                    let val_start = pos;
                    while pos < len && bytes[pos] != b'"' {
                        pos += 1;
                    }
                    if pos >= len {
                        return Err(format!("unterminated string in array for key '{}'", key));
                    }
                    let val = s[val_start..pos].to_string();
                    pos += 1; // skip closing '"'
                    items.push(val);
                    // 공백 건너뜀
                    while pos < len
                        && (bytes[pos] == b' '
                            || bytes[pos] == b'\t'
                            || bytes[pos] == b'\r'
                            || bytes[pos] == b'\n')
                    {
                        pos += 1;
                    }
                    if pos < len && bytes[pos] == b',' {
                        pos += 1; // skip ','
                    }
                } else if bytes[pos] == b']' {
                    pos += 1;
                    break;
                } else {
                    return Err(format!(
                        "unexpected character in array for key '{}': {:?}",
                        key, bytes[pos] as char
                    ));
                }
            }
            map.insert(key.to_string(), TomlValue::Array(items));
        } else {
            return Err(format!(
                "expected string or array for key '{}', found {:?}",
                key, bytes[pos] as char
            ));
        }

        // 줄 끝까지 건너뜀 (주석 포함)
        while pos < len && bytes[pos] != b'\n' {
            pos += 1;
        }
    }

    Ok(map)
}

impl AppManifest {
    /// TOML 문자열로부터 파싱.
    pub fn from_toml(s: &str) -> Result<Self, ManifestError> {
        let map = parse_minimal_toml(s).map_err(ManifestError::Toml)?;

        // `id` 필드는 필수
        let id = match map.get("id") {
            Some(TomlValue::Str(v)) => v.clone(),
            Some(TomlValue::Array(_)) => {
                return Err(ManifestError::Toml("'id' must be a string, not an array".to_string()));
            }
            None => {
                return Err(ManifestError::Toml("missing field 'id'".to_string()));
            }
        };

        let permissions = match map.get("permissions") {
            Some(TomlValue::Array(v)) => v.clone(),
            Some(TomlValue::Str(_)) => {
                return Err(ManifestError::Toml("'permissions' must be an array".to_string()));
            }
            None => Vec::new(),
        };

        let ui_type_strs: Vec<String> = match map.get("ui_types") {
            Some(TomlValue::Array(v)) => v.clone(),
            Some(TomlValue::Str(_)) => {
                return Err(ManifestError::Toml("'ui_types' must be an array".to_string()));
            }
            None => Vec::new(),
        };

        let mut ui_types = Vec::new();
        for t in ui_type_strs {
            let parsed = TypeUri::parse(&t).map_err(|_| ManifestError::BadTypeUri(t.clone()))?;
            ui_types.push(parsed);
        }

        Ok(Self { id, permissions, ui_types })
    }

    /// TOML 문자열로 직렬화.
    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let mut out = String::new();
        out.push_str(&format!("id = {:?}\n", self.id));

        // permissions array
        out.push_str("permissions = [");
        for (i, p) in self.permissions.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{:?}", p));
        }
        out.push_str("]\n");

        // ui_types array
        out.push_str("ui_types = [");
        for (i, t) in self.ui_types.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{:?}", t.as_str()));
        }
        out.push_str("]\n");

        Ok(out)
    }

    /// 주어진 type_uri가 매니페스트에 선언되어 있는지.
    pub fn declares_type(&self, type_uri: &TypeUri) -> bool {
        self.ui_types.iter().any(|t| t == type_uri)
    }
}
