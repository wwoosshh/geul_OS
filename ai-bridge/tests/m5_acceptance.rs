//! M5 acceptance — MockAdapter로 결정론 e2e.
//!
//! 실제 Claude API 호출은 사용자가 수동으로:
//!   ANTHROPIC_API_KEY=... cargo run -p geulos-ai-bridge -- run \
//!     --scenario ai-bridge/scenarios/01_explore.toml

use geulos_ai_bridge::adapter::{LlmResponse, LlmStop, MockAdapter, ToolUse};
use geulos_ai_bridge::session::{Session, SessionBudget};
use geulos_ai_bridge::WireClient;
use geulos_core::{std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern};
use geulos_proto::EventKindFilterWire;
use geulos_server_host::run_listener;
use serde_json::json;

/// M5 acceptance: AI agent가 와이어 프로토콜로 다단계 작업 완수.
/// 시나리오: 발견 → 호출 → 구독 → drain → 종료. probe.py의 가장 풍부한 흐름.
#[tokio::test]
async fn m5_acceptance_full_press_subscribe_drain_flow() {
    // 1. 서버 띄우기 + 사전 mount된 버튼 (wildcard ACL)
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut mounter = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    // 2. 사전 subscribe 클라이언트 (이벤트 관찰자) — *외부 ai로 관찰*
    let mut observer = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let sub_id =
        observer.subscribe(&btn_id, &[EventKindFilterWire::Invoke]).await.unwrap();

    // 3. AI agent (Mock) — 발견 → press → 종료
    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mock = MockAdapter::new(vec![
        LlmResponse {
            text: vec!["Find Button.".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-1".to_string(),
                name: "list_objects_by_type".to_string(),
                input: json!({"type_uri": "aios.std/Button@1"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (100, 30),
        },
        LlmResponse {
            text: vec!["Press it.".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-2".to_string(),
                name: "invoke_method".to_string(),
                input: json!({"target": btn_id, "method": "press", "args": null}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (200, 40),
        },
        LlmResponse {
            text: vec!["Done.".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-3".to_string(),
                name: "report_done".to_string(),
                input: json!({"summary": "Found and pressed button successfully"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (250, 30),
        },
    ]);

    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 5, ..Default::default() });

    let outcome = session.run_task("Press the button.").await.unwrap();

    // 4. 검증 — 세션 완료 + 관찰자가 invoke 이벤트 수신
    assert!(outcome.completed);
    assert_eq!(outcome.summary.as_deref(), Some("Found and pressed button successfully"));
    assert_eq!(outcome.turns_used, 3);
    assert!(outcome.input_tokens > 0);
    assert!(outcome.output_tokens > 0);

    // 잠시 대기 후 관찰자 drain — invoke 이벤트가 와 있어야 함
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let events = observer.drain(&sub_id).await.unwrap();
    assert!(!events.is_empty(), "observer가 press의 Invoke 이벤트를 받아야 함");
}

/// M5 budget enforcement — report_done 없이 turn 소진하면 incomplete.
#[tokio::test]
async fn m5_acceptance_budget_enforced_when_done_not_called() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mut endless_responses = Vec::new();
    for i in 0..20 {
        endless_responses.push(LlmResponse {
            text: vec![format!("turn {}", i)],
            tool_uses: vec![ToolUse {
                id: format!("tu-{}", i),
                name: "list_objects_by_type".to_string(),
                input: json!({"type_uri": "aios.std/Text@1"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (50, 10),
        });
    }
    let mock = MockAdapter::new(endless_responses);

    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 4, ..Default::default() });
    let outcome = session.run_task("loop").await.unwrap();
    assert!(!outcome.completed, "budget 소진 시 incomplete여야 함");
    assert_eq!(outcome.turns_used, 5); // 4 다음 turn에서 break
}
