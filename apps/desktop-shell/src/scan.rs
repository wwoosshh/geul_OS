//! 워크스페이스 디렉터리 → Folder/File 객체 트리 스캔.
//!
//! 단방향 동기화: 디스크 → 객체. 숨김 파일·`.git`/`node_modules`/`target`·`.vs`/`.idea`
//! 같은 노이즈 디렉터리는 제외. 텍스트 파일은 앞 512바이트를 UTF-8 경계 안전하게
//! 잘라 `preview` state에 담음.

use std::path::Path;

use geulos_core::{std_types, ActorId, Object, ObjectId};

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".vs", ".idea"];

const TEXT_EXTS: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("toml", "text/plain"),
    ("json", "text/json"),
    ("rs", "text/rust"),
    ("py", "text/python"),
    ("js", "text/javascript"),
    ("html", "text/html"),
    ("css", "text/css"),
    ("yaml", "text/yaml"),
    ("yml", "text/yaml"),
];

/// 스캔 결과. 모든 발견된 객체(Folder + File)를 평탄한 리스트로 담는다.
///
/// Folder 객체는 `children`에 직계 자식 ObjectId들을 갖고, 각 자식 객체는
/// `parent`에 Folder의 ObjectId를 갖는다. 워크스페이스 *루트*의 직계 자식은
/// `parent`가 `None`인 상태로 반환 — main에서 FileTree id로 다시 채움.
pub struct ScanResult {
    /// 모든 발견된 객체(Folder + File 평탄 리스트).
    pub objects: Vec<Object>,
}

/// 디렉터리를 재귀적으로 스캔.
pub fn scan_tree(owner: &ActorId, root: &Path) -> std::io::Result<ScanResult> {
    let mut out = Vec::new();
    walk(owner, root, &mut out)?;
    Ok(ScanResult { objects: out })
}

/// 한 디렉터리를 훑고, 그 안에서 새로 만들어진 *직계* 객체들의 ObjectId 목록을 반환.
fn walk(owner: &ActorId, dir: &Path, out: &mut Vec<Object>) -> std::io::Result<Vec<ObjectId>> {
    let mut child_ids = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        // 권한 부족 등으로 못 읽으면 *조용히* 빈 자식 반환. 스캔 전체를 깨뜨리지 않음.
        Err(_) => return Ok(child_ids),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let created_ms = chrono::Utc::now().timestamp_millis();
            let mut folder = std_types::folder(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                created_ms,
            );
            let nested = walk(owner, &path, out)?;
            folder.state.insert("child_count".into(), serde_json::json!(nested.len()));
            // 자식들의 parent를 이 Folder의 id로 갱신.
            for id in &nested {
                if let Some(child) = out.iter_mut().find(|o| o.id == *id) {
                    child.parent = Some(folder.id);
                }
            }
            folder.children = nested;
            child_ids.push(folder.id);
            out.push(folder);
        } else if ft.is_file() {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let mime = TEXT_EXTS
                .iter()
                .find(|(e, _)| *e == ext)
                .map(|(_, m)| *m)
                .unwrap_or("application/octet-stream");
            let created_ms = chrono::Utc::now().timestamp_millis();
            // `file`은 std_types::file와 충돌해 shadowing 발생 → `file_obj`로 분리.
            let mut file_obj = std_types::file(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                mime,
                created_ms,
            );
            if let Ok(meta) = std::fs::metadata(&path) {
                file_obj.state.insert("size_bytes".into(), serde_json::json!(meta.len()));
            }
            if mime.starts_with("text/") {
                if let Ok(bytes) = std::fs::read(&path) {
                    let cap = bytes.len().min(512);
                    let safe = utf8_safe_slice(&bytes, cap);
                    if let Ok(s) = std::str::from_utf8(safe) {
                        file_obj.state.insert("preview".into(), serde_json::json!(s));
                    }
                }
            }
            child_ids.push(file_obj.id);
            out.push(file_obj);
        }
    }
    Ok(child_ids)
}

/// UTF-8 경계를 침범하지 않는 가장 긴 prefix를 반환.
///
/// 마지막 바이트가 연속 바이트(10xxxxxx)면 그 시퀀스를 끝까지 자르고,
/// 그 위치의 *선두* 바이트(11xxxxxx)도 함께 잘라낸다.
fn utf8_safe_slice(bytes: &[u8], max: usize) -> &[u8] {
    let mut end = max.min(bytes.len());
    while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] >= 0b1100_0000 {
        end -= 1;
    }
    &bytes[..end]
}
