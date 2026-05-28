//! ConsoleWindow id ↔ JobHandle (Windows) 매핑 — in-process HashMap.
//!
//! handle_terminate / dialog_methods의 ConsoleTerminate arm / exit waiter task가
//! 공통으로 lookup. Arc<Mutex<_>>로 spawn task와 main loop 모두 접근.

use std::collections::HashMap;
use std::sync::Arc;

use geulos_core::ObjectId;
use tokio::sync::Mutex;

use crate::job_object::JobHandle;

#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<HashMap<ObjectId, JobHandle>>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 새 (ConsoleWindow id → JobHandle) 등록.
    ///
    /// 기존 매핑이 있으면 *덮어쓰기* — 반환된 old `JobHandle`이 statement 끝에 drop되며
    /// `CloseHandle` 호출 → `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 발화 → *이전 ConsoleWindow의
    /// process tree 전체 즉시 kill*. v1에서 새 ConsoleWindow id는 매번 UUID 신규 발급이라
    /// 도달 X — 그러나 T5 미래 refactor에서 같은 id 재사용 시 의도치 않은 cascade kill 위험.
    pub async fn insert(&self, id: ObjectId, job: JobHandle) {
        self.inner.lock().await.insert(id, job);
    }

    /// 매핑 *제거* — handle 반환 (호출자가 drop 책임). exit waiter task가 정상 종료
    /// 시 호출 — drop이 CloseHandle 실행하지만 process는 이미 죽었으니 cascade kill no-op.
    #[must_use = "drop 시 CloseHandle → KILL_ON_JOB_CLOSE로 process tree kill 가능 — 의도적으로 무시하려면 let _ = ..."]
    pub async fn remove(&self, id: ObjectId) -> Option<JobHandle> {
        self.inner.lock().await.remove(&id)
    }

    /// terminate 호출 — 매핑이 있으면 TerminateJobObject. 매핑 제거는 exit waiter
    /// task가 child.wait() 종료 후 별도로 처리.
    pub async fn terminate(&self, id: ObjectId) -> Result<(), String> {
        let guard = self.inner.lock().await;
        let job = guard.get(&id).ok_or_else(|| format!("ConsoleWindow {} 매핑 없음", id))?;
        job.terminate().map_err(|e| format!("TerminateJobObject 실패: {}", e))
    }

    #[must_use]
    pub async fn contains(&self, id: ObjectId) -> bool {
        self.inner.lock().await.contains_key(&id)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_remove_roundtrip() {
        let reg = ProcessRegistry::new();
        let id = ObjectId::new();
        let job = JobHandle::create().expect("create");
        reg.insert(id, job).await;
        assert!(reg.contains(id).await);
        let _ = reg.remove(id).await.expect("remove");
        assert!(!reg.contains(id).await);
    }

    #[tokio::test]
    async fn terminate_unknown_id_returns_err() {
        let reg = ProcessRegistry::new();
        let result = reg.terminate(ObjectId::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn terminate_does_not_remove_entry() {
        // 설계 계약: terminate()는 TerminateJobObject만 호출 + map entry 보존.
        // map 제거는 exit waiter task가 child.wait() 종료 후 별도 remove() 호출.
        // 이 invariant가 깨지면 (예: terminate 내부에 remove 추가) exit waiter가
        // None 받아 silently 처리 누락 → handle Drop이 일찍 발화 → cascade kill timing 어긋남.
        let reg = ProcessRegistry::new();
        let id = ObjectId::new();
        let job = JobHandle::create().expect("create");
        reg.insert(id, job).await;
        let _ = reg.terminate(id).await; // 빈 job → Ok이지만 Err여도 entry 보존 검증이 본 의도
        assert!(
            reg.contains(id).await,
            "terminate()는 map entry를 제거하지 말아야 함 — exit waiter remove() 책임"
        );
    }
}
