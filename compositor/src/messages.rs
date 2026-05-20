//! winit 메인 스레드와 tokio TCP 스레드 사이의 메시지 정의.

use geulos_core::{Object, ObjectId};
use serde_json::Value;

/// winit → tokio: 클릭/입력 등에 의해 발생한 액션.
#[derive(Debug, Clone)]
pub enum UiAction {
    /// 객체의 메서드 호출 요청.
    Invoke { target: ObjectId, method: String, args: Value },
    /// 종료 요청.
    Quit,
    /// M8 T8.17: scroll_y 같은 *순수 시각 상태* 직접 SetState — invoke 우회로 latency 절감.
    ///
    /// move/resize/focus/close 같은 *의미론적* 액션은 여전히 Invoke (서버측 핸들러가
    /// 정책/검증 수행). scroll_y는 viewer-only 상태라 *컴포지터가 직접 SetState*. 매 휠
    /// 이벤트가 invoke → handler → SetState 라운드트립을 거치지 않도록 한다.
    SetState { target: ObjectId, key: String, value: Value },
}

/// tokio → winit: 서버에서 받은 변화.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// 객체가 (재)등록됨 — TreeModel.upsert.
    ObjectUpserted(Object),
    /// 객체가 사라짐.
    ObjectRemoved(ObjectId),
    /// 객체의 state 키 갱신됨.
    StateSet { id: ObjectId, key: String, value: Value },
    /// 연결 손실.
    Disconnected,
}
