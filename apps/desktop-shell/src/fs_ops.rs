//! 단방향 디스크 기록 — 객체 변경(create_file / write / delete)을 디스크에 반영.
//!
//! M7 단방향 동기화 원칙: 객체 → 디스크. 반대 방향(FS watcher → 객체)은 M9+.
//!
//! Atomic write 전략: `<path>.tmp.geulos`에 먼저 기록 후 `rename`. POSIX/Windows 모두
//! rename은 atomic (또는 atomic에 매우 가까움)이라 reader가 부분 기록을 보지 않는다.
//!
//! safe_join: AI/외부 액터가 `..`를 끼워 workspace 밖으로 탈출하는 시도를 차단.

use std::path::{Component, Path, PathBuf};

/// 빈 파일을 생성. 부모 디렉터리가 없으면 재귀 생성.
///
/// 이미 존재하면 truncate (File::create 의미). T7 시점에서 create_file 호출 시
/// 같은 이름 파일이 있으면 덮어쓰는 것이 자연스러우므로 별도 검사 없음.
pub fn create_empty_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(path)?;
    Ok(())
}

/// 바이트를 atomic write. 같은 디렉터리의 `<name>.tmp.geulos`에 먼저 쓰고 rename.
///
/// 부모 디렉터리는 없으면 자동 생성. 같은 partition 안의 rename은 POSIX·NTFS
/// 둘 다 사실상 atomic — reader가 `old` 또는 `new`만 보고 부분 기록은 안 본다.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp.geulos");
    std::fs::write(&tmp, bytes)?;
    // Windows에서 target이 존재할 때 rename이 거부되는 케이스가 있어 fallback.
    // 일반적으로 std::fs::rename은 NTFS에서 replace 의미를 갖지만, 권한 등으로
    // 실패할 수 있어 명시적 remove → rename 으로 재시도.
    if let Err(e) = std::fs::rename(&tmp, path) {
        if path.exists() {
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path)?;
        } else {
            // tmp 파일은 청소하고 원래 에러를 그대로 전파.
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    Ok(())
}

/// 파일을 삭제. 없으면 NotFound 에러를 그대로 전파.
pub fn delete_file(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// `base + rel`을 join하되 `..` 컴포넌트가 base를 탈출하면 에러.
///
/// 알고리즘:
/// 1. `base`와 `rel`을 합친 뒤 컴포넌트를 순회.
/// 2. 누적된 경로 길이가 base의 컴포넌트 수 미만으로 떨어지면 탈출로 판정.
///
/// `std::fs::canonicalize`는 *실제로 존재하는* 경로에서만 동작하므로 (생성 *전*에
/// 검증해야 하는 우리 케이스에 부적합) 직접 컴포넌트 수준에서 검증한다.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let base_depth = base.components().count();
    let mut acc: PathBuf = base.to_path_buf();
    let mut current_depth = base_depth;
    for c in Path::new(rel).components() {
        match c {
            Component::ParentDir => {
                if current_depth <= base_depth {
                    return Err(format!(
                        "path traversal: '{}' 이(가) '{}' 밖을 가리킴",
                        rel,
                        base.display()
                    ));
                }
                acc.pop();
                current_depth -= 1;
            }
            Component::CurDir => {}
            Component::Normal(seg) => {
                acc.push(seg);
                current_depth += 1;
            }
            // rel 안에 절대 경로 prefix(예: 윈도우 드라이브 문자)나 RootDir가 있으면
            // base와 무관한 경로가 되어버리므로 거부.
            Component::Prefix(_) | Component::RootDir => {
                return Err(format!("path traversal: '{}' 가 절대 경로 컴포넌트 포함", rel));
            }
        }
    }
    Ok(acc)
}
