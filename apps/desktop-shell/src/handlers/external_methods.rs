//! Filesystem@1 escape hatch — read_external / write_external (M10 Phase 3 / ADR-036).
//!
//! cwd *밖* 임의 path 접근을 위한 escape hatch. cwd 안은 거부 (Folder@1/File@1
//! 객체-네이티브 흐름 사용 권장). read는 부수효과 없으므로 Dialog 없이 즉시,
//! write는 *매 호출* Dialog confirm (cwd 밖이라 dir grant 모델 적용 X).

use std::path::{Path, PathBuf};

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingMap};
use crate::handlers::add_dialog_acl;
use crate::invoke_handler::InvokeOutcome;

/// Filesystem@1.read_external(path) — cwd *밖* 임의 path read. cwd 안은 거부.
/// v1 단순화: read는 부수효과 없으므로 Dialog 없이 즉시 통과. 결과는
/// state.last_read_path/last_read_content로 SetState broadcast → AI가 후속
/// get_object로 본문 확인.
pub fn handle_read_external(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
    filesystem_id: ObjectId,
    cwd: &Path,
) -> InvokeOutcome {
    if target_id != filesystem_id {
        // 다른 객체에 read_external을 보내면 무시 (Filesystem@1 singleton 전용).
        return InvokeOutcome::empty();
    }
    let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let path = PathBuf::from(&path_str);
    if path_str.is_empty() {
        eprintln!("[desktop-shell] read_external: 빈 path 무시");
        return InvokeOutcome::empty();
    }
    if path.starts_with(cwd) {
        // cwd 안 — 객체-네이티브 흐름이 정답. AI에게 *기계 가독 신호*를 state로 보냄.
        // 이전엔 eprintln + empty outcome으로 silent fail이었음 → AI가 도구 동작 여부만
        // 알고 이유는 추측 → max_inner_turns 헛걸음 (사용자 보고). 본 fix는 state에
        // 명시 에러 메시지를 SetState로 broadcast — AI가 후속 get_object/subscribe에서
        // 정확한 이유 + fallback 흐름 안내를 받는다.
        let msg = format!(
            "ERROR cwd-inside: '{}' 는 cwd 안 경로입니다. read_external은 cwd 밖 전용. \
             cwd 안 파일은 list_objects_by_type('aios.std/File@1') 후 props.path가 \
             일치하는 객체에 invoke_method(<file_id>, 'read', {{}})를 호출하세요.",
            path.display()
        );
        eprintln!("[desktop-shell] read_external 거부 — {}", msg);
        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == filesystem_id) {
            o.state.insert("last_read_path".into(), json!(&path_str));
            o.state.insert("last_read_content".into(), json!(&msg));
        }
        return InvokeOutcome {
            state_sets: vec![
                (filesystem_id, "last_read_path".to_string(), json!(path_str)),
                (filesystem_id, "last_read_content".to_string(), json!(msg)),
            ],
        };
    }
    // cwd 밖 — 즉시 read OK (read-only, 부수효과 없음).
    // VM 빌드면 호스트 경로(C:\...)는 host bridge로 라우팅 — VM이 직접 Windows path를 read하면 ENOENT.
    #[cfg(not(windows))]
    let read_result: Result<String, String> = {
        if crate::host_bridge_client::is_host_path(&path_str) {
            const MAX: u64 = 1 << 20;
            match crate::host_bridge_client::read_file(&path_str, MAX) {
                Some((bytes, _truncated)) => match String::from_utf8(bytes) {
                    Ok(s) => Ok(s),
                    Err(e) => Err(format!("UTF-8 디코딩 실패: {}", e)),
                },
                None => Err("호스트 브리지 read_file 실패".to_string()),
            }
        } else {
            std::fs::read_to_string(&path).map_err(|e| e.to_string())
        }
    };
    #[cfg(windows)]
    let read_result: Result<String, String> =
        std::fs::read_to_string(&path).map_err(|e| e.to_string());

    match read_result {
        Ok(content) => {
            let size = content.len();
            // mounted_objects의 Filesystem state도 동기화.
            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == filesystem_id) {
                o.state.insert("last_read_path".into(), json!(&path_str));
                o.state.insert("last_read_content".into(), json!(&content));
            }
            eprintln!("[desktop-shell] read_external OK ({} bytes) → {}", size, path.display());
            InvokeOutcome {
                state_sets: vec![
                    (filesystem_id, "last_read_path".to_string(), json!(path_str)),
                    (filesystem_id, "last_read_content".to_string(), json!(content)),
                ],
            }
        }
        Err(e) => {
            eprintln!("[desktop-shell] read_external 실패 {}: {}", path.display(), e);
            // AI에게 에러 이유를 state로 명시 (silent fail 회피).
            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == filesystem_id) {
                o.state.insert("last_read_path".into(), json!(&path_str));
                o.state.insert("last_read_content".into(), json!(format!("ERROR read: {}", e)));
            }
            InvokeOutcome {
                state_sets: vec![
                    (filesystem_id, "last_read_path".to_string(), json!(path_str)),
                    (filesystem_id, "last_read_content".to_string(), json!(format!("ERROR read: {}", e))),
                ],
            }
        }
    }
}

/// Filesystem@1.write_external(path, content) — cwd *밖* 임의 path write.
/// *매 호출* Dialog confirm — cwd 밖이라 dir grant 모델 적용 X (위험도 항상 높음).
/// cwd 안은 거부 + 안내 (Folder@1.create_file / File@1.save 사용 권장).
#[allow(clippy::too_many_arguments)]
pub async fn handle_write_external(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    filesystem_id: ObjectId,
    cwd: &Path,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    if target_id != filesystem_id {
        return Ok(InvokeOutcome::empty());
    }
    let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let path = PathBuf::from(&path_str);
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if path_str.is_empty() {
        eprintln!("[desktop-shell] write_external: 빈 path 무시");
        return Ok(InvokeOutcome::empty());
    }
    if path.starts_with(cwd) {
        // cwd 안 — 객체-네이티브 (Folder.create_file / File.save). state로 명시 안내.
        let msg = format!(
            "ERROR cwd-inside: '{}' 는 cwd 안 경로입니다. write_external은 cwd 밖 전용. \
             cwd 안 파일은 list_objects_by_type('aios.std/Folder@1' 또는 'aios.std/File@1') 후 \
             해당 객체에 invoke_method(<id>, 'create_file' 또는 'save', ...)를 호출하세요.",
            path.display()
        );
        eprintln!("[desktop-shell] write_external 거부 — {}", msg);
        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == filesystem_id) {
            // last_read_content를 재활용 — schema 변경 없이 AI에게 즉시 신호.
            o.state.insert("last_read_path".into(), json!(&path_str));
            o.state.insert("last_read_content".into(), json!(&msg));
        }
        return Ok(InvokeOutcome {
            state_sets: vec![
                (filesystem_id, "last_read_path".to_string(), json!(path_str)),
                (filesystem_id, "last_read_content".to_string(), json!(msg)),
            ],
        });
    }
    // cwd 밖 — 항상 Dialog. 사용자 응답을 respond 분기에서 PendingMap.take.
    let mut dialog = std_types::dialog(
        owner.clone(),
        "AI 외부 경로 write 확인",
        &format!("AI가 cwd 밖 경로 {} 에 write 시도합니다. 허용?", path.display()),
        "warn",
        vec!["허용".to_string(), "거부".to_string()],
    );
    dialog.parent = Some(desktop_id);
    add_dialog_acl(&mut dialog);
    let dialog_id = dialog.id;

    let mm =
        MountMsg { root_object_id: dialog_id.to_string(), tree: serde_json::to_value(&dialog)? };
    stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
    *req_seq += 1;
    let sub = SubscribeMsg {
        subscription_id: format!("sub-runtime-{}", req_seq),
        target: dialog_id.to_string(),
        kinds: vec![EventKindFilterWire::Invoke],
        include_initial: false,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
    mounted_objects.push(dialog);

    let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
    pending.insert(
        dialog_id,
        dialog_ops::PendingEntry {
            op: dialog_ops::PendingFs::ExternalWrite {
                path,
                content,
                requesting_actor: sender_actor.clone(),
            },
            tx,
        },
    );
    eprintln!("[desktop-shell] AI write_external Dialog mount (target {}): 응답 대기", dialog_id);
    Ok(InvokeOutcome::empty())
}
