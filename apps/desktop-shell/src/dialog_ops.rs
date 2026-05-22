//! Dialog@1 mount/respond + Pending(actor 응답 대기) 매핑 (M9 / ADR-035).
//!
//! AI write가 ConfirmRequired면 Dialog mount + 원래 save args를 PendingSave에 보관.
//! 사용자가 Dialog.respond("허용"/"거부")로 응답하면 결과를 oneshot으로 깨워준다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use geulos_core::ObjectId;
use tokio::sync::oneshot;

/// AI invoke 응답을 기다리는 한 건. file_id/path/content와 깨움 채널.
pub struct PendingSave {
    pub file_id: ObjectId,
    pub path: PathBuf,
    pub content: String,
    /// Dialog 응답이 도착하면 보내는 채널.
    /// payload: 사용자가 클릭한 라벨 ("허용" / "거부" 등).
    pub tx: oneshot::Sender<String>,
}

/// dialog_id → PendingSave 매핑.
#[derive(Default)]
pub struct PendingMap {
    inner: Mutex<HashMap<ObjectId, PendingSave>>,
}

impl PendingMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, dialog_id: ObjectId, p: PendingSave) {
        self.inner.lock().expect("PendingMap poisoned").insert(dialog_id, p);
    }

    pub fn take(&self, dialog_id: ObjectId) -> Option<PendingSave> {
        self.inner.lock().expect("PendingMap poisoned").remove(&dialog_id)
    }

    pub fn contains(&self, dialog_id: ObjectId) -> bool {
        self.inner.lock().expect("PendingMap poisoned").contains_key(&dialog_id)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("PendingMap poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_take_round_trip() {
        let map = PendingMap::new();
        let dialog_id = ObjectId::new();
        let file_id = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            dialog_id,
            PendingSave { file_id, path: PathBuf::from("/x"), content: "y".into(), tx },
        );
        assert!(map.contains(dialog_id));
        assert_eq!(map.len(), 1);
        let taken = map.take(dialog_id).expect("present");
        assert_eq!(taken.file_id, file_id);
        assert!(!map.contains(dialog_id));
    }

    #[test]
    fn take_missing_returns_none() {
        let map = PendingMap::new();
        assert!(map.take(ObjectId::new()).is_none());
    }

    #[tokio::test]
    async fn respond_wakes_oneshot() {
        let map = PendingMap::new();
        let dialog_id = ObjectId::new();
        let (tx, rx) = oneshot::channel();
        map.insert(
            dialog_id,
            PendingSave {
                file_id: ObjectId::new(),
                path: PathBuf::from("/x"),
                content: "z".into(),
                tx,
            },
        );
        let p = map.take(dialog_id).expect("present");
        p.tx.send("허용".to_string()).expect("send");
        let got = rx.await.expect("recv");
        assert_eq!(got, "허용");
    }
}
