//! ObjectServer 액터.
//!
//! ObjectServer는 단일 라이터 모델(ADR-003)이라 비동기 환경에서 직접 공유 불가.
//! mpsc 채널로 명령을 받아 직렬 처리하는 *액터 패턴*으로 노출.

use geulos_core::{
    ActorId, Event, EventKindFilter, InvokeError, MountError, Object, ObjectId, ObjectServer,
    Query, SubscriptionId,
};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// 액터 핸들. 복제 가능(`Arc` 내부).
#[derive(Clone)]
pub struct ObjectServerHandle {
    tx: mpsc::Sender<Command>,
}

/// 액터에 보내는 명령.
enum Command {
    Mount {
        obj: Object,
        reply: oneshot::Sender<Result<ObjectId, MountError>>,
    },
    Invoke {
        actor: ActorId,
        target: ObjectId,
        method: String,
        args: Value,
        reply: oneshot::Sender<Result<geulos_core::EventId, InvokeError>>,
    },
    Get {
        id: ObjectId,
        reply: oneshot::Sender<Option<Object>>,
    },
    Query {
        q: Query,
        reply: oneshot::Sender<Vec<ObjectId>>,
    },
    Subscribe {
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
        reply: oneshot::Sender<SubscriptionId>,
    },
    Unsubscribe {
        id: SubscriptionId,
    },
    Drain {
        id: SubscriptionId,
        reply: oneshot::Sender<Vec<Event>>,
    },
}

/// 핸들 호출 에러.
#[derive(Debug, Error)]
pub enum HandleError {
    /// 액터가 종료됨.
    #[error("actor task gone")]
    ActorGone,
    /// 호출 실패 (코어 에러).
    #[error("{0}")]
    Core(String),
}

impl ObjectServerHandle {
    /// Mount.
    pub async fn mount(&self, obj: Object) -> Result<ObjectId, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::Mount { obj, reply: tx })
            .await
            .map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)?.map_err(|e| HandleError::Core(e.to_string()))
    }

    /// Invoke.
    pub async fn invoke(
        &self,
        actor: ActorId,
        target: ObjectId,
        method: String,
        args: Value,
    ) -> Result<geulos_core::EventId, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::Invoke { actor, target, method, args, reply: tx })
            .await
            .map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)?.map_err(|e| HandleError::Core(e.to_string()))
    }

    /// 객체 가져오기 (복사본).
    pub async fn get(&self, id: ObjectId) -> Result<Option<Object>, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Command::Get { id, reply: tx }).await.map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)
    }

    /// Query.
    pub async fn query(&self, q: Query) -> Result<Vec<ObjectId>, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Command::Query { q, reply: tx }).await.map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)
    }

    /// Subscribe.
    pub async fn subscribe(
        &self,
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> Result<SubscriptionId, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Command::Subscribe { subscriber, target, filters, reply: tx })
            .await
            .map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)
    }

    /// Unsubscribe.
    pub async fn unsubscribe(&self, id: SubscriptionId) -> Result<(), HandleError> {
        self.tx.send(Command::Unsubscribe { id }).await.map_err(|_| HandleError::ActorGone)
    }

    /// 구독 큐 비우기.
    pub async fn drain(&self, id: SubscriptionId) -> Result<Vec<Event>, HandleError> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Command::Drain { id, reply: tx }).await.map_err(|_| HandleError::ActorGone)?;
        rx.await.map_err(|_| HandleError::ActorGone)
    }
}

/// ObjectServer 액터 — 한 task가 ObjectServer를 단독 소유.
pub struct ObjectServerActor;

impl ObjectServerActor {
    /// 액터를 spawn하고 핸들을 반환.
    pub fn spawn() -> ObjectServerHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(64);
        tokio::spawn(async move {
            let mut server = ObjectServer::new();
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Mount { obj, reply } => {
                        let res = server.mount(obj);
                        let _ = reply.send(res);
                    }
                    Command::Invoke { actor, target, method, args, reply } => {
                        let res = server.invoke(&actor, &target, &method, args);
                        let _ = reply.send(res);
                    }
                    Command::Get { id, reply } => {
                        let _ = reply.send(server.get(&id).cloned());
                    }
                    Command::Query { q, reply } => {
                        let _ = reply.send(server.query(&q));
                    }
                    Command::Subscribe { subscriber, target, filters, reply } => {
                        let id = server.subscribe(subscriber, target, filters);
                        let _ = reply.send(id);
                    }
                    Command::Unsubscribe { id } => {
                        server.unsubscribe(id);
                    }
                    Command::Drain { id, reply } => {
                        let _ = reply.send(server.drain_subscription(id));
                    }
                }
            }
        });
        ObjectServerHandle { tx }
    }
}
