//! Session 매니저 통합 테스트 — MockAdapter로 결정론 실행.

use geulos_ai_bridge::adapter::{LlmResponse, LlmStop, MockAdapter, ToolUse};
use geulos_ai_bridge::session::{Session, SessionBudget};
use geulos_ai_bridge::WireClient;
use geulos_server_host::run_listener;
use serde_json::json;

#[tokio::test]
async fn session_with_mock_calls_report_done_immediately() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let mock = MockAdapter::new(vec![LlmResponse {
        text: vec!["I'll report immediately.".to_string()],
        tool_uses: vec![ToolUse {
            id: "tu-1".to_string(),
            name: "report_done".to_string(),
            input: json!({"summary": "test summary"}),
        }],
        stop: LlmStop::ToolUse,
        tokens: (100, 20),
    }]);

    let mut session = Session::new(mock, wire, "You are a test.".to_string())
        .with_budget(SessionBudget { max_turns: 5, ..Default::default() });

    let outcome = session.run_task("just do it").await.unwrap();
    assert!(outcome.completed);
    assert_eq!(outcome.summary.as_deref(), Some("test summary"));
    assert_eq!(outcome.turns_used, 1);
    assert_eq!(outcome.input_tokens, 100);
    assert_eq!(outcome.output_tokens, 20);
}

#[tokio::test]
async fn session_respects_max_turns_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    // Mock이 *영원히* list_objects 호출만 함 — report_done 없음.
    let mut responses = Vec::new();
    for i in 0..10 {
        responses.push(LlmResponse {
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
    let mock = MockAdapter::new(responses);

    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 3, ..Default::default() });

    let outcome = session.run_task("loop").await.unwrap();
    assert!(!outcome.completed, "report_done 없이 종료해야 함");
    // 4 = 3을 *넘어선* 시점에 break
    assert_eq!(outcome.turns_used, 4);
}

#[tokio::test]
async fn session_full_press_and_observe_flow() {
    use geulos_core::{std_types, ActorId};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    // 사전: 버튼 mount (wildcard ACL)
    let mut mounter = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(geulos_core::AclEntry {
        actor: geulos_core::ActorPattern::Wildcard,
        method: geulos_core::MethodPattern::Wildcard,
        effect: geulos_core::AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    // Mock 시나리오:
    //   turn 1: list_objects_by_type(Button) → ID 받음
    //   turn 2: invoke press
    //   turn 3: report_done
    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mock = MockAdapter::new(vec![
        LlmResponse {
            text: vec!["Find button.".to_string()],
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
                input: json!({"summary": "Pressed the button."}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (250, 30),
        },
    ]);

    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 5, ..Default::default() });

    let outcome = session.run_task("Press a button.").await.unwrap();
    assert!(outcome.completed);
    assert_eq!(outcome.summary.as_deref(), Some("Pressed the button."));
    assert_eq!(outcome.turns_used, 3);
}
