//! desktop-shell 진입점 (T2: 스캐폴드 + 워크스페이스 확보까지).
//!
//! T3에서 server-host 연결 + 스캔 + mount 추가.

use geulos_desktop_shell::workspace;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace::resolve()?;
    workspace::ensure_exists(&root)?;
    println!("[desktop-shell] workspace root: {}", root.display());
    println!("[desktop-shell] T2 scaffold complete — T3에서 스캔·mount 추가");
    Ok(())
}
