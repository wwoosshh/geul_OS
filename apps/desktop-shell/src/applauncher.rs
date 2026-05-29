//! 앱 레지스트리 — app_id를 "어떤 앱인지"로 해석 (순수 디스패치).
//! Desktop.launch / Dock.launch / DesktopIcon.open 이 공통 사용 → 사용자·AI 동일 경로.

/// 알려진 앱 종류.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AppKind {
    FileManager,
    // SP3: Notepad, ...
}

/// app_id가 알려진 앱이면 그 종류 반환. 알 수 없으면 None.
pub fn resolve_app(app_id: &str) -> Option<AppKind> {
    match app_id {
        "file_manager" => Some(AppKind::FileManager),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_app_resolves() {
        assert_eq!(resolve_app("file_manager"), Some(AppKind::FileManager));
    }
    #[test]
    fn unknown_app_is_none() {
        assert_eq!(resolve_app("nope"), None);
    }
}
