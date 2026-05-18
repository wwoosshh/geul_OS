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
    let ext = std::path::Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("");
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
