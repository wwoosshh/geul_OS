//! GeulOS core crate.

pub mod event;
pub mod object;
pub mod server;
pub use object::std_types;

pub use event::{Event, EventBus, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorIdParseError, ActorPattern, AppManifest, ArgSpec, EventId,
    ManifestError, MethodPattern, MethodSig, Object, ObjectId, TypeUri,
};
pub use server::{EventKindFilter, InvokeError, MountError, ObjectServer, Query, SubscriptionId};
