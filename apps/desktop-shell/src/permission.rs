//! write 권한 정책 — `actor + op -> Allow | ConfirmRequired` (M9 / ADR-035).
//!
//! v1: 사용자 직접 Save는 Allow, 그 외 모두 ConfirmRequired. M10에서 create/delete/rename으로
//! Op 확장 + `judge_with_path` (디렉터리 단위 grant 모델 — ADR-036). 표는 spec §"권한 정책" 참고.

use geulos_core::ActorId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Save,
    /// M10: 파일 생성 (Folder.create_file).
    CreateFile,
    /// M10: 폴더 생성 (Folder.create_folder).
    CreateFolder,
    Delete,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    ConfirmRequired,
}

/// `actor`가 `op`를 수행할 때 다이얼로그 confirm이 필요한지 판정 (path-blind).
///
/// v1 정책:
/// - 사용자(local-user) Save: Allow
/// - 사용자 Delete: ConfirmRequired (M10에서 작동)
/// - 그 외 사용자 op (CreateFile/CreateFolder/Rename): Allow
/// - 그 외 모든 actor (ai 등) 모든 op: ConfirmRequired
///
/// path-aware 판정은 `judge_with_path` 사용 (M10 Phase 1).
pub fn judge(actor: &ActorId, op: Op) -> Verdict {
    let is_local_user = actor == &ActorId::local_user();
    match (is_local_user, op) {
        (true, Op::Save) => Verdict::Allow,
        (true, Op::CreateFile) => Verdict::Allow,
        (true, Op::CreateFolder) => Verdict::Allow,
        (true, Op::Rename) => Verdict::Allow,
        (true, Op::Delete) => Verdict::ConfirmRequired,
        (false, _) => Verdict::ConfirmRequired,
    }
}

use std::path::Path;

use crate::granted_dirs::GrantedDirs;

/// path-aware 권한 판정 (M10 Phase 1 / ADR-036).
///
/// 디렉터리 단위 grant 모델:
/// - 사용자 (local-user): UI 자체가 confirm이라 항상 Allow.
/// - AI Delete: granted 무관 항상 ConfirmRequired (위험).
/// - AI Save/CreateFile/CreateFolder/Rename: granted_dirs에 해당 dir 있으면 Allow,
///   없으면 ConfirmRequired (Dialog 후 사용자 허용 시 main이 dir grant 추가).
///
/// `dir`은 작업 대상 *디렉터리 경로* — File.save라면 file의 parent dir, Folder.create_file
/// 이라면 folder.path. main.rs가 호출 시 정확한 dir을 전달.
pub fn judge_with_path(actor: &ActorId, op: Op, dir: &Path, granted: &GrantedDirs) -> Verdict {
    let is_local_user = actor == &ActorId::local_user();
    if is_local_user {
        return Verdict::Allow;
    }
    if op == Op::Delete {
        return Verdict::ConfirmRequired;
    }
    if granted.contains(dir) {
        Verdict::Allow
    } else {
        Verdict::ConfirmRequired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::granted_dirs::GrantedDirs;
    use std::path::{Path, PathBuf};

    fn ai() -> ActorId {
        // ai-bridge가 connection.rs에서 ActorId::new_ai_session()으로 발급 (Role::Ai).
        ActorId::new_ai_session()
    }

    #[test]
    fn user_save_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::Save), Verdict::Allow);
    }

    #[test]
    fn user_create_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::CreateFile), Verdict::Allow);
    }

    #[test]
    fn user_create_folder_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::CreateFolder), Verdict::Allow);
    }

    #[test]
    fn user_rename_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::Rename), Verdict::Allow);
    }

    #[test]
    fn user_delete_requires_confirm() {
        assert_eq!(judge(&ActorId::local_user(), Op::Delete), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_save_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Save), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_create_requires_confirm() {
        assert_eq!(judge(&ai(), Op::CreateFile), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_create_folder_requires_confirm() {
        assert_eq!(judge(&ai(), Op::CreateFolder), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_delete_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Delete), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_rename_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Rename), Verdict::ConfirmRequired);
    }

    #[test]
    fn user_always_allowed_path() {
        let g = GrantedDirs::new();
        let user = ActorId::local_user();
        assert_eq!(judge_with_path(&user, Op::Save, Path::new("/x"), &g), Verdict::Allow);
        assert_eq!(judge_with_path(&user, Op::Delete, Path::new("/x"), &g), Verdict::Allow);
        assert_eq!(judge_with_path(&user, Op::CreateFile, Path::new("/x"), &g), Verdict::Allow);
    }

    #[test]
    fn ai_delete_always_confirm_path() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/x"));
        let ai_actor = ai();
        assert_eq!(
            judge_with_path(&ai_actor, Op::Delete, Path::new("/x"), &g),
            Verdict::ConfirmRequired
        );
    }

    #[test]
    fn ai_save_in_granted_dir_allowed() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/x"));
        let ai_actor = ai();
        assert_eq!(judge_with_path(&ai_actor, Op::Save, Path::new("/x"), &g), Verdict::Allow);
        assert_eq!(judge_with_path(&ai_actor, Op::CreateFile, Path::new("/x"), &g), Verdict::Allow);
        assert_eq!(judge_with_path(&ai_actor, Op::Rename, Path::new("/x"), &g), Verdict::Allow);
    }

    #[test]
    fn ai_save_in_ungranted_dir_confirm() {
        let g = GrantedDirs::new();
        let ai_actor = ai();
        assert_eq!(
            judge_with_path(&ai_actor, Op::Save, Path::new("/x"), &g),
            Verdict::ConfirmRequired
        );
        assert_eq!(
            judge_with_path(&ai_actor, Op::CreateFolder, Path::new("/x"), &g),
            Verdict::ConfirmRequired
        );
    }

    #[test]
    fn ai_grant_is_per_dir() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        let ai_actor = ai();
        assert_eq!(judge_with_path(&ai_actor, Op::Save, Path::new("/a"), &g), Verdict::Allow);
        assert_eq!(
            judge_with_path(&ai_actor, Op::Save, Path::new("/b"), &g),
            Verdict::ConfirmRequired
        );
    }
}
