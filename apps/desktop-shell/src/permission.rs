//! write 권한 정책 — `actor + op -> Allow | ConfirmRequired` (M9 / ADR-035).
//!
//! v1: 사용자 직접 Save는 Allow, 그 외 모두 ConfirmRequired. M10에서 create/delete/rename으로
//! Op 확장. 표는 spec §"권한 정책" 참고.

use geulos_core::ActorId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Save,
    /// M10 예약 — v1은 사용 안 함.
    Create,
    Delete,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    ConfirmRequired,
}

/// `actor`가 `op`를 수행할 때 다이얼로그 confirm이 필요한지 판정.
///
/// v1 정책:
/// - 사용자(local-user) Save: Allow
/// - 사용자 Delete: ConfirmRequired (M10에서 작동)
/// - 그 외 사용자 op (Create/Rename): Allow
/// - 그 외 모든 actor (ai 등) 모든 op: ConfirmRequired
pub fn judge(actor: &ActorId, op: Op) -> Verdict {
    let is_local_user = actor == &ActorId::local_user();
    match (is_local_user, op) {
        (true, Op::Save) => Verdict::Allow,
        (true, Op::Create) => Verdict::Allow,
        (true, Op::Rename) => Verdict::Allow,
        (true, Op::Delete) => Verdict::ConfirmRequired,
        (false, _) => Verdict::ConfirmRequired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(judge(&ActorId::local_user(), Op::Create), Verdict::Allow);
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
        assert_eq!(judge(&ai(), Op::Create), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_delete_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Delete), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_rename_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Rename), Verdict::ConfirmRequired);
    }
}
