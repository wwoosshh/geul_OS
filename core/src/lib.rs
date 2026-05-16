//! GeulOS core crate.

pub mod event;
pub mod object;

pub use event::{Event, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    Object, ObjectId, TypeUri,
};
