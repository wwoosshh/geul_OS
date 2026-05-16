//! mount(): 객체 서브트리 등록.

use thiserror::Error;

use crate::event::{EventKind, LifecycleKind};
use crate::object::{Object, ObjectId};
use crate::server::ObjectServer;

/// mount 실패 사유.
#[derive(Debug, Error)]
pub enum MountError {
    /// 같은 ID를 가진 객체가 이미 존재.
    #[error("이미 등록된 ObjectId: {0}")]
    DuplicateId(ObjectId),
}

impl ObjectServer {
    /// 단일 객체를 루트로 등록한다.
    pub fn mount(&mut self, obj: Object) -> Result<ObjectId, MountError> {
        if self.objects.contains_key(&obj.id) {
            return Err(MountError::DuplicateId(obj.id));
        }
        let id = obj.id;
        let owner = obj.owner.clone();
        self.objects.insert(id, obj);
        self.roots.push(id);
        self.bus.emit(owner, id, EventKind::Lifecycle(LifecycleKind::Created), None);
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }
        Ok(id)
    }

    /// 루트와 그 자손들을 한꺼번에 등록한다.
    ///
    /// `descendants`의 객체들은 `root.children`에서 참조되는 순서대로 와야 한다.
    /// 각 자손도 Created 이벤트가 발생한다.
    pub fn mount_with_descendants(
        &mut self,
        root: Object,
        descendants: Vec<Object>,
    ) -> Result<ObjectId, MountError> {
        // 먼저 중복 검사
        if self.objects.contains_key(&root.id) {
            return Err(MountError::DuplicateId(root.id));
        }
        for d in &descendants {
            if self.objects.contains_key(&d.id) {
                return Err(MountError::DuplicateId(d.id));
            }
        }

        let root_id = root.id;
        let root_owner = root.owner.clone();

        // 등록 & 이벤트 발행
        self.objects.insert(root_id, root);
        self.roots.push(root_id);
        self.bus.emit(root_owner, root_id, EventKind::Lifecycle(LifecycleKind::Created), None);
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }

        for d in descendants {
            let id = d.id;
            let owner = d.owner.clone();
            self.objects.insert(id, d);
            self.bus.emit(owner, id, EventKind::Lifecycle(LifecycleKind::Created), None);
            if let Some(ev) = self.bus.log().last() {
                self.subscriptions.deliver(ev);
            }
        }

        Ok(root_id)
    }
}
