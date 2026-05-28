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

    /// 새 (ConsoleWindow id → JobHandle) 등록. 기존 매핑 있으면 *덮어쓰기*.
    pub async fn insert(&self, id: ObjectId, job: JobHandle) {
        self.inner.lock().await.insert(id, job);
    }

    /// 매핑 *제거* — handle 반환 (호출자가 drop 책임). exit waiter task가 정상 종료
    /// 시 호출 — drop이 CloseHandle 실행하지만 process는 이미 죽었으니 cascade kill no-op.
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
}
