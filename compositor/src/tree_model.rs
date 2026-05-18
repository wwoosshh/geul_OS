//! 컴포지터가 보유하는 로컬 객체 트리 미러.
//!
//! server-host에서 받은 Object들의 *복사본*을 ObjectId로 인덱스. 입력 hit-test와
//! 렌더링은 이 미러를 읽음. 서버 이벤트가 도착하면 미러 업데이트.

use std::collections::HashMap;

use geulos_core::{Object, ObjectId, TypeUri};

/// 로컬 트리 모델.
#[derive(Debug, Default)]
pub struct TreeModel {
    objects: HashMap<ObjectId, Object>,
    /// 컴포지터가 처음 query로 발견한 루트 후보 (parent가 None인 것).
    roots: Vec<ObjectId>,
    /// 부모가 아직 도착하지 않은 자식들: parent_id → [waiting child ids].
    /// type-subscribe Created가 부모보다 자식이 먼저 도착하는 race를 해소 —
    /// 부모가 upsert될 때 그 리스트를 drain해서 children에 push.
    orphans: HashMap<ObjectId, Vec<ObjectId>>,
}

impl TreeModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 객체 한 개 삽입 또는 덮어쓰기.
    ///
    /// parent가 있는 객체면 *부모.children에 자동 push* — 단 중복 회피.
    /// desktop-shell의 lazy_mount는 자식만 mount하고 부모.children Vec은 갱신을
    /// 별도 wire 메시지로 보내지 않으므로 (children은 state map이 아닌 Object 필드
    /// 라 SetState 불가), 컴포지터 측에서 자식의 `parent` 필드를 보고 자동으로
    /// 부모 트리에 등록한다. 이렇게 해야 layout이 부모.children iterate로 자식을
    /// 찾는다.
    pub fn upsert(&mut self, obj: Object) {
        let id = obj.id;
        let parent_opt = obj.parent;
        let is_root = obj.parent.is_none();
        self.objects.insert(id, obj);
        if is_root && !self.roots.contains(&id) {
            self.roots.push(id);
        }
        // 1. 새 객체가 *부모로서* 기다리고 있는 orphan 자식들이 있으면 채워준다.
        if let Some(waiting) = self.orphans.remove(&id) {
            if let Some(this) = self.objects.get_mut(&id) {
                for child_id in waiting {
                    if !this.children.contains(&child_id) {
                        this.children.push(child_id);
                    }
                }
            }
        }
        // 2. 새 객체가 *자식*이면 부모.children에 push 또는 orphan 등록.
        if let Some(parent_id) = parent_opt {
            if let Some(parent) = self.objects.get_mut(&parent_id) {
                if !parent.children.contains(&id) {
                    parent.children.push(id);
                }
            } else {
                // 부모가 아직 안 옴 — orphan으로 등록. 부모가 나중에 upsert되면 retrofit.
                let entry = self.orphans.entry(parent_id).or_default();
                if !entry.contains(&id) {
                    entry.push(id);
                }
            }
        }
    }

    /// 객체 제거 (Lifecycle Destroyed에 대응).
    pub fn remove(&mut self, id: ObjectId) {
        self.objects.remove(&id);
        self.roots.retain(|r| *r != id);
    }

    /// 객체 조회.
    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// 모든 ID 순회.
    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects.keys().copied()
    }

    /// 루트 목록.
    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    /// 객체 개수.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// 비어있는지.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// state 키 갱신 (StateSet 이벤트 처리용).
    pub fn set_state(&mut self, id: ObjectId, key: String, value: serde_json::Value) {
        if let Some(obj) = self.objects.get_mut(&id) {
            obj.state.insert(key, value);
        }
    }

    /// 특정 타입 URI의 객체만 추리기.
    pub fn objects_of_type(&self, type_uri: &TypeUri) -> Vec<ObjectId> {
        self.objects.iter().filter(|(_, o)| &o.type_uri == type_uri).map(|(id, _)| *id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geulos_core::{std_types, ActorId};

    /// lazy_mount 자식이 컴포지터에 자동으로 부모.children에 등록되는지 검증.
    /// 이게 안 되면 자식 객체가 트리에 있어도 layout이 부모.children iterate에서 못 찾음.
    #[test]
    fn upsert_child_auto_registers_in_parent_children() {
        let owner = ActorId::local_user();
        let parent = std_types::folder(owner.clone(), "/p", "p", 0);
        let parent_id = parent.id;
        let mut child = std_types::folder(owner.clone(), "/p/c", "c", 0);
        child.parent = Some(parent_id);
        let child_id = child.id;

        let mut tree = TreeModel::new();
        tree.upsert(parent);
        assert!(tree.get(parent_id).unwrap().children.is_empty());
        tree.upsert(child);
        assert_eq!(tree.get(parent_id).unwrap().children, vec![child_id]);
    }

    #[test]
    fn upsert_child_dedup_does_not_duplicate() {
        let owner = ActorId::local_user();
        let mut parent = std_types::folder(owner.clone(), "/p", "p", 0);
        let parent_id = parent.id;
        let mut child = std_types::folder(owner.clone(), "/p/c", "c", 0);
        child.parent = Some(parent_id);
        let child_id = child.id;
        // 부모가 이미 children에 child_id 가지고 mount된 경우 (기존 일괄 mount 패턴).
        parent.children.push(child_id);

        let mut tree = TreeModel::new();
        tree.upsert(parent);
        tree.upsert(child);
        // 중복 없이 한 번만.
        assert_eq!(tree.get(parent_id).unwrap().children, vec![child_id]);
    }

    #[test]
    fn upsert_root_no_parent_registers_as_root() {
        let owner = ActorId::local_user();
        let d = std_types::desktop(owner);
        let id = d.id;
        let mut tree = TreeModel::new();
        tree.upsert(d);
        assert_eq!(tree.roots(), &[id]);
    }

    /// 자식이 부모보다 *먼저* 도착한 경우, 부모가 나중에 upsert될 때 retrofit되어야 한다.
    /// type-subscribe Created가 부모와 자식 사이 순서를 보장하지 않으므로 필수.
    #[test]
    fn child_before_parent_retrofits_on_parent_upsert() {
        let owner = ActorId::local_user();
        let parent = std_types::folder(owner.clone(), "/p", "p", 0);
        let parent_id = parent.id;
        let mut child = std_types::folder(owner.clone(), "/p/c", "c", 0);
        child.parent = Some(parent_id);
        let child_id = child.id;

        let mut tree = TreeModel::new();
        // 자식 먼저 upsert — 부모 없으니 orphan으로 park.
        tree.upsert(child);
        assert!(tree.get(parent_id).is_none());
        // 부모 도착 — orphan retrofit.
        tree.upsert(parent);
        assert_eq!(tree.get(parent_id).unwrap().children, vec![child_id]);
    }

    /// 여러 자식이 같은 부모를 기다리다가 부모 도착 시 한꺼번에 retrofit.
    #[test]
    fn multiple_orphans_retrofit_on_parent_arrival() {
        let owner = ActorId::local_user();
        let parent = std_types::folder(owner.clone(), "/p", "p", 0);
        let parent_id = parent.id;
        let mut child1 = std_types::folder(owner.clone(), "/p/a", "a", 0);
        child1.parent = Some(parent_id);
        let mut child2 = std_types::folder(owner.clone(), "/p/b", "b", 0);
        child2.parent = Some(parent_id);
        let mut child3 = std_types::file(owner.clone(), "/p/c.txt", "c.txt", "text/plain", 0);
        child3.parent = Some(parent_id);

        let mut tree = TreeModel::new();
        tree.upsert(child1.clone());
        tree.upsert(child2.clone());
        tree.upsert(child3.clone());
        tree.upsert(parent);

        let children = &tree.get(parent_id).unwrap().children;
        assert_eq!(children.len(), 3);
        assert!(children.contains(&child1.id));
        assert!(children.contains(&child2.id));
        assert!(children.contains(&child3.id));
    }
}
