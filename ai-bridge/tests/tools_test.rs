//! Tools dispatch 통합 테스트.

use geulos_ai_bridge::tools::{dispatch_tool, standard_tools, DispatchResult};
use geulos_ai_bridge::WireClient;
use geulos_server_host::run_listener;
use serde_json::json;

#[tokio::test]
async fn standard_tools_includes_six_functions() {
    let tools = standard_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"list_objects_by_type"));
    assert!(names.contains(&"get_object"));
    assert!(names.contains(&"invoke_method"));
    assert!(names.contains(&"subscribe"));
    assert!(names.contains(&"drain"));
    assert!(names.contains(&"report_done"));
    assert_eq!(tools.len(), 6);
}

#[tokio::test]
async fn report_done_returns_done_variant() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    let mut wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let r = dispatch_tool(&mut wire, "report_done", &json!({"summary": "done"})).await.unwrap();
    assert!(matches!(r, DispatchResult::Done { .. }));
}

#[tokio::test]
async fn list_objects_returns_output_with_object_ids_field() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    let mut wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let r =
        dispatch_tool(&mut wire, "list_objects_by_type", &json!({"type_uri": "aios.std/Text@1"}))
            .await
            .unwrap();
    match r {
        DispatchResult::Output(v) => {
            assert!(v.get("object_ids").is_some());
        }
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn unknown_tool_returns_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    let mut wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let r = dispatch_tool(&mut wire, "no_such_tool", &json!({})).await;
    assert!(r.is_err());
}
