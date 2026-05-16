//! 객체 관련 타입과 ID 정의.

pub mod acl;
pub mod identity;
pub mod method;

pub use acl::{AclEffect, AclEntry, ActorPattern, MethodPattern};
pub use identity::{ActorId, EventId, ObjectId, TypeUri};
pub use method::{ArgSpec, MethodSig};
