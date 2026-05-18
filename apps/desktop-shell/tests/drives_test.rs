use geulos_desktop_shell::drives;

#[test]
fn list_drives_returns_at_least_one_path() {
    let ds = drives::list_drives();
    assert!(!ds.is_empty(), "최소 한 드라이브는 있어야 함");
    for d in &ds {
        assert!(d.exists() || cfg!(not(windows)), "{} 가 실제 디렉터리여야 함", d.display());
    }
}

#[cfg(windows)]
#[test]
fn list_drives_includes_drive_letter() {
    let ds = drives::list_drives();
    let paths: Vec<String> = ds.iter().map(|p| p.display().to_string()).collect();
    // C: 또는 D: 중 하나는 존재한다고 가정 (Windows 통상 환경).
    assert!(
        paths.iter().any(|p| p.starts_with("C:") || p.starts_with("D:")),
        "C:\\ 또는 D:\\ 중 하나는 있어야: {:?}",
        paths
    );
}
