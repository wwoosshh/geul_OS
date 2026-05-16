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
}

impl TreeModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 객체 한 개 삽입 또는 덮어쓰기.
    pub fn upsert(&mut self, obj: Object) {
        let id = obj.id;
        let is_root = obj.parent.is_none();
        self.objects.insert(id, obj);
        if is_root && !self.roots.contains(&id) {
            self.roots.push(id);
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
        self.objects
            .iter()
            .filter(|(_, o)| &o.type_uri == type_uri)
            .map(|(id, _)| *id)
            .collect()
    }
}
