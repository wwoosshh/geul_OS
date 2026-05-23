//! Dialog@1 mount/respond + Pending (사용자 응답 대기) 매핑 (M9/M10).
//!
//! M10 Phase 1 확장: 한 Dialog가 *다양한 fs 작업* (save/create_file/create_folder/delete/
//! rename)에 대응. PendingFs enum이 그 종류를 카테고리화.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use geulos_core::ObjectId;
use tokio::sync::oneshot;

/// pending 작업의 종류. 사용자가 Dialog에 응답하면 desktop-shell이 이 enum을 보고 분기.
#[derive(Debug)]
pub enum PendingFs {
    /// File@1.save — args.content를 디스크에 commit. file_id로 path lookup.
    Save { file_id: ObjectId, path: PathBuf, content: String },
    /// Folder@1.create_file — folder 안에 새 빈 파일.
    CreateFile { folder_id: ObjectId, folder_path: PathBuf, name: String },
    /// Folder@1.create_folder — folder 안에 새 빈 폴더.
    CreateFolder { folder_id: ObjectId, folder_path: PathBuf, name: String },
    /// File@1.delete — 파일 자체 삭제.
    DeleteFile { file_id: ObjectId, path: PathBuf },
    /// Folder@1.delete — 폴더 자체 삭제 (recursive flag).
    DeleteFolder { folder_id: ObjectId, path: PathBuf, recursive: bool },
    /// File@1.rename or Folder@1.rename.
    Rename { target_id: ObjectId, path: PathBuf, new_name: String, is_folder: bool },
}

pub struct PendingEntry {
    pub op: PendingFs,
    pub tx: oneshot::Sender<String>,
}

#[derive(Default)]
pub struct PendingMap {
    inner: Mutex<HashMap<ObjectId, PendingEntry>>,
}

impl PendingMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, dialog_id: ObjectId, entry: PendingEntry) {
        self.inner.lock().expect("PendingMap poisoned").insert(dialog_id, entry);
    }

    pub fn take(&self, dialog_id: ObjectId) -> Option<PendingEntry> {
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
    fn insert_take_save_entry() {
        let map = PendingMap::new();
        let did = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            did,
            PendingEntry {
                op: PendingFs::Save {
                    file_id: ObjectId::new(),
                    path: PathBuf::from("/x"),
                    content: "y".into(),
                },
                tx,
            },
        );
        assert!(map.contains(did));
        let taken = map.take(did).expect("present");
        assert!(matches!(taken.op, PendingFs::Save { .. }));
    }

    #[test]
    fn insert_take_create_file_entry() {
        let map = PendingMap::new();
        let did = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            did,
            PendingEntry {
                op: PendingFs::CreateFile {
                    folder_id: ObjectId::new(),
                    folder_path: PathBuf::from("/p"),
                    name: "x.txt".into(),
                },
                tx,
            },
        );
        let taken = map.take(did).expect("present");
        match taken.op {
            PendingFs::CreateFile { name, .. } => assert_eq!(name, "x.txt"),
            _ => panic!("expected CreateFile"),
        }
    }

    #[test]
    fn take_missing_returns_none() {
        let map = PendingMap::new();
        assert!(map.take(ObjectId::new()).is_none());
    }
}
