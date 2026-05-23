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
    /// 단일 객체 등록.
    ///
    /// `obj.parent`이 None이면 *루트*로 추가. Some(parent_id)이고 그 부모가 이미 서버에
    /// 등록되어 있으면 부모.children에 자동 push (중복 회피, idempotent). 부모가 아직
    /// 없으면 무시 — *서버 트리에 매달리지 못한 orphan*이지만 후속 `get`은 가능
    /// (호출자 책임). 트리 정합성은 호출자가 유지하는 게 원칙.
    ///
    /// **M10 결함 2 fix:** 이전에는 *모든* mount가 무조건 `roots`에 push되고 부모.children
    /// 갱신이 없어, desktop-shell이 자식만 mount한 후 server측 store는 `parent.children=[]`
    /// 상태로 남았다. AI가 `get(parent)`로 children을 조회하면 빈 배열 → "빈 폴더" 오답.
    /// `parent.children.push` + `roots` 등록 회피로 desktop-shell 패턴과 server store가
    /// 동기화된다.
    pub fn mount(&mut self, obj: Object) -> Result<ObjectId, MountError> {
        if self.objects.contains_key(&obj.id) {
            return Err(MountError::DuplicateId(obj.id));
        }
        let id = obj.id;
        let owner = obj.owner.clone();
        let parent_opt = obj.parent;
        // type_uri는 객체를 map에 옮기기 전에 캐싱 (deliver 시 ByType 매칭에 사용).
        let type_uri = obj.type_uri.clone();
        self.objects.insert(id, obj);
        match parent_opt {
            None => self.roots.push(id),
            Some(parent_id) => {
                if let Some(parent) = self.objects.get_mut(&parent_id) {
                    if !parent.children.contains(&id) {
                        parent.children.push(id);
                    }
                } else {
                    // 부모 미등록 — 호환을 위해 일단 roots로 fallback (legacy 동작).
                    // 이러면 트리에서 고립되지만 get/query는 동작.
                    self.roots.push(id);
                }
            }
        }
        self.bus.emit(owner, id, EventKind::Lifecycle(LifecycleKind::Created), None);
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev, Some(&type_uri));
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
        let root_type = root.type_uri.clone();

        // 등록 & 이벤트 발행
        self.objects.insert(root_id, root);
        self.roots.push(root_id);
        self.bus.emit(root_owner, root_id, EventKind::Lifecycle(LifecycleKind::Created), None);
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev, Some(&root_type));
        }

        for d in descendants {
            let id = d.id;
            let owner = d.owner.clone();
            let type_uri = d.type_uri.clone();
            self.objects.insert(id, d);
            self.bus.emit(owner, id, EventKind::Lifecycle(LifecycleKind::Created), None);
            if let Some(ev) = self.bus.log().last() {
                self.subscriptions.deliver(ev, Some(&type_uri));
            }
        }

        Ok(root_id)
    }
}
