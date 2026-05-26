//! GeulOS 와이어 프로토콜 타입.

pub mod codec;
pub mod handshake;
pub mod messages;

pub use codec::{decode_frame, encode_frame, DecodeError, MAX_FRAME_SIZE};
pub use handshake::{Hello, HelloAck, HelloReject, Role};
pub use messages::{
    EventKindFilterWire, EventMsg, GetError, GetMsg, GetResult, GlscriptError, GlscriptMsg,
    GrantOp, GrantUpdate, InvokeAck, InvokeError, InvokeMsg, MountAck, MountMsg, MountReject,
    QueryMsg, QueryPredicate, QueryResult, StateSetAck, StateSetError, StateSetMsg, SubscribeAck,
    SubscribeMsg, UnsubscribeMsg,
};
