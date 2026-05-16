//! query(): 객체 트리 단발 조회.

use crate::object::{ActorId, ObjectId, TypeUri};
use crate::server::ObjectServer;

/// 조회 조건.
#[derive(Debug, Clone)]
pub enum Query {
    /// 특정 타입의 모든 객체.
    ByType(TypeUri),
    /// 특정 액터가 소유한 모든 객체.
    ByOwner(ActorId),
    /// 특정 부모의 직계 자식들.
    ChildrenOf(ObjectId),
}

impl Query {
    /// 타입 기준.
    pub fn by_type(t: TypeUri) -> Self {
        Self::ByType(t)
    }

    /// 소유자 기준.
    pub fn by_owner(a: ActorId) -> Self {
        Self::ByOwner(a)
    }

    /// 자식 기준.
    pub fn children_of(parent: ObjectId) -> Self {
        Self::ChildrenOf(parent)
    }
}

impl ObjectServer {
    /// 트리에서 조건에 맞는 객체 ID 목록을 반환한다.
    pub fn query(&self, q: &Query) -> Vec<ObjectId> {
        match q {
            Query::ByType(t) => {
                self.objects.iter().filter(|(_, o)| &o.type_uri == t).map(|(id, _)| *id).collect()
            }
            Query::ByOwner(a) => {
                self.objects.iter().filter(|(_, o)| &o.owner == a).map(|(id, _)| *id).collect()
            }
            Query::ChildrenOf(parent) => {
                self.objects.get(parent).map(|o| o.children.clone()).unwrap_or_default()
            }
        }
    }
}
