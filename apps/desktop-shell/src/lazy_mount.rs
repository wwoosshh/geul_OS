//! 폴더 expand 시 직계 자식 mount. M8 ADR-028 — 전체 재귀 mount는 비현실 (메모리).

use std::io;
use std::path::Path;

use geulos_core::{std_types, ActorId, Object};

/// `folder_path` 직계 자식 (Folder + File) 객체 목록을 반환.
///
/// 권한 거부 / 경로 없음 등 io 에러는 빈 vec로 silent (M8 trade-off, ADR-028).
/// 반환된 객체의 `parent`는 None — 호출자가 부모 ObjectId로 채워야 함.
pub fn expand_folder(owner: &ActorId, folder_path: &Path, now_ms: i64) -> io::Result<Vec<Object>> {
    let entries = match std::fs::read_dir(folder_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!(
                "[lazy_mount] read_dir 실패 {}: {} — 빈 폴더로 처리",
                folder_path.display(),
                e
            );
            return Ok(Vec::new());
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue, // 비-UTF8 이름은 skip
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let obj = if meta.is_dir() {
            std_types::folder(owner.clone(), path.to_string_lossy().as_ref(), &name, now_ms)
        } else if meta.is_file() {
            let mime = guess_mime(&name);
            let mut f = std_types::file(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                mime,
                now_ms,
            );
            // size_bytes만 빠르게 채움 — preview는 클릭 시점에 (M9? 또는 별 enrich).
            f.set_state("size_bytes", serde_json::json!(meta.len()));
            f
        } else {
            continue; // 심볼릭 링크 등 skip
        };
        out.push(obj);
    }
    Ok(out)
}

fn guess_mime(name: &str) -> &'static str {
    // T8.19: 흔한 dotfile은 확장자가 없거나 비표준 — 명시적 화이트리스트로 text 매핑.
    // binary detection (read 후 UTF-8 check)은 비용 + 가짜 양성 위험이 있어 v1은 이름
    // 매칭만. 새 dotfile 종류는 이 리스트에 추가.
    match name {
        ".env" | ".envrc" | ".gitignore" | ".gitattributes" | ".dockerignore" | ".editorconfig"
        | ".prettierrc" | ".eslintrc" => return "text/plain",
        _ => {}
    }

    let ext = std::path::Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.is_empty() {
        // 확장자가 없는 흔한 텍스트 파일은 화이트리스트로 cover (README, LICENSE 등).
        // 그 외 확장자 없는 파일은 binary로 가정해 application/octet-stream.
        match name {
            "README" | "LICENSE" | "Makefile" | "Dockerfile" | "Cargo" => return "text/plain",
            _ => return "application/octet-stream",
        }
    }

    match ext.to_ascii_lowercase().as_str() {
        "txt" | "log" | "ini" | "cfg" | "toml" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" => "text/json",
        "rs" => "text/rust",
        "py" => "text/python",
        "js" | "ts" => "text/javascript",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "yaml" | "yml" => "text/yaml",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_mime_dotfiles_return_text_plain() {
        assert_eq!(guess_mime(".env"), "text/plain");
        assert_eq!(guess_mime(".envrc"), "text/plain");
        assert_eq!(guess_mime(".gitignore"), "text/plain");
        assert_eq!(guess_mime(".gitattributes"), "text/plain");
        assert_eq!(guess_mime(".dockerignore"), "text/plain");
        assert_eq!(guess_mime(".editorconfig"), "text/plain");
        assert_eq!(guess_mime(".prettierrc"), "text/plain");
        assert_eq!(guess_mime(".eslintrc"), "text/plain");
    }

    #[test]
    fn guess_mime_known_extension_files_return_text() {
        assert_eq!(guess_mime("hello.txt"), "text/plain");
        assert_eq!(guess_mime("README.md"), "text/markdown");
        assert_eq!(guess_mime("Cargo.toml"), "text/plain");
        assert_eq!(guess_mime("config.yaml"), "text/yaml");
        assert_eq!(guess_mime("lib.rs"), "text/rust");
    }

    #[test]
    fn guess_mime_unknown_returns_octet_stream() {
        assert_eq!(guess_mime("photo.png"), "application/octet-stream");
        assert_eq!(guess_mime("weird.xyz"), "application/octet-stream");
        assert_eq!(guess_mime("unknown_no_ext"), "application/octet-stream");
        assert_eq!(guess_mime("archive.zip"), "application/octet-stream");
    }

    #[test]
    fn guess_mime_known_no_extension_files_return_text() {
        assert_eq!(guess_mime("README"), "text/plain");
        assert_eq!(guess_mime("LICENSE"), "text/plain");
        assert_eq!(guess_mime("Makefile"), "text/plain");
        assert_eq!(guess_mime("Dockerfile"), "text/plain");
    }
}
