//! Dialog@1.respond — 사용자 confirm/deny 응답 처리 (M9 T8 / M10 T7 / Phase 3).
//!
//! 사용자가 [허용]/[거부] 클릭. PendingMap.take → 분기에 따라 적절한 fs operation
//! 실행 + 객체 mount/destroy/state 갱신 + (Create/Rename은) granted_dirs 추가 + Dialog
//! destroy (KI-011 tombstone 패턴).
//!
//! M10 T7: PendingFs 모든 variant 처리 (Save/CreateFile/CreateFolder/DeleteFile/
//! DeleteFolder/Rename/ExternalWrite). Create*/Rename/Save 승인 시 부모 dir grant 추가 →
//! 같은 dir 안 후속 동일 actor 작업은 confirm 생략 (per-dir TOFU).

use geulos_core::{ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingMap};
use crate::fs_watcher::FsWatcher;
use crate::granted_dirs::{self, GrantedDirs};
use crate::handlers::add_fs_object_acl;
use crate::invoke_handler::InvokeOutcome;
use crate::{file_ops, file_write, folder_ops};

#[allow(clippy::too_many_arguments)]
pub async fn handle_respond(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    pending: &PendingMap,
    granted: &GrantedDirs,
    fs_watcher: Option<&FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("거부").to_string();
    let pending_entry = pending.take(target_id);
    // M10 결함 4 fix: DeleteFile/DeleteFolder 승인 시 *target 객체*의 destroyed
    // tombstone을 wire에 broadcast해야 compositor 트리에서 옛 객체가 사라진다.
    // 이전엔 local state만 갱신하고 wire 전송이 누락 → 사용자 화면에 옛 이름
    // 잔존. Dialog destroyed broadcast 와 함께 outcome.state_sets에 같이 push.
    let mut extra_state_sets: Vec<(ObjectId, String, serde_json::Value)> = Vec::new();
    if let Some(entry) = pending_entry {
        if action == "허용" {
            let now = chrono::Utc::now().timestamp_millis();
            match entry.op {
                dialog_ops::PendingFs::Save { path, content, requesting_actor, .. } => {
                    // M10 Phase 2: echo 표시 — Dialog 승인 후 fs op 직전.
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(path.clone());
                    }
                    match file_write::save(&path, &content) {
                        Ok(()) => {
                            eprintln!(
                                "[desktop-shell] AI save 승인 → {} 저장 완료",
                                path.display()
                            );
                            // Save도 dir grant — 같은 dir 후속 write 자유 (ADR-036
                            // 모델 일관). M9는 path-blind judge였어서 매번 confirm.
                            // M11: grant_dir helper로 local + server 동시 동기화.
                            if let Some(parent) = path.parent() {
                                let _ = granted_dirs::grant_dir(
                                    granted,
                                    stream,
                                    &requesting_actor,
                                    parent.to_path_buf(),
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI save (응답 후) 실패: {}", e);
                        }
                    }
                }
                dialog_ops::PendingFs::CreateFile {
                    folder_id,
                    folder_path,
                    name,
                    requesting_actor,
                } => {
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(folder_path.join(&name));
                    }
                    match folder_ops::create_file_in(owner, &folder_path, &name, now) {
                        Ok(mut new_obj) => {
                            new_obj.parent = Some(folder_id);
                            add_fs_object_acl(&mut new_obj);
                            let new_id = new_obj.id;
                            let mm = MountMsg {
                                root_object_id: new_id.to_string(),
                                tree: serde_json::to_value(&new_obj)?,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                            *req_seq += 1;
                            let sub = SubscribeMsg {
                                subscription_id: format!("sub-runtime-{}", req_seq),
                                target: new_id.to_string(),
                                kinds: vec![EventKindFilterWire::Invoke],
                                include_initial: false,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                            if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == folder_id)
                            {
                                p.children.push(new_id);
                            }
                            mounted_objects.push(new_obj);
                            eprintln!(
                                "[desktop-shell] AI create_file 승인 → {}/{}",
                                folder_path.display(),
                                name
                            );
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI create_file (응답 후) 실패: {}", e);
                        }
                    }
                    // M11: grant_dir helper로 local + server 동시 동기화.
                    let _ =
                        granted_dirs::grant_dir(granted, stream, &requesting_actor, folder_path)
                            .await;
                }
                dialog_ops::PendingFs::CreateFolder {
                    folder_id,
                    folder_path,
                    name,
                    requesting_actor,
                } => {
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(folder_path.join(&name));
                    }
                    match folder_ops::create_folder_in(owner, &folder_path, &name, now) {
                        Ok(mut new_obj) => {
                            new_obj.parent = Some(folder_id);
                            add_fs_object_acl(&mut new_obj);
                            let new_id = new_obj.id;
                            let mm = MountMsg {
                                root_object_id: new_id.to_string(),
                                tree: serde_json::to_value(&new_obj)?,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                            *req_seq += 1;
                            let sub = SubscribeMsg {
                                subscription_id: format!("sub-runtime-{}", req_seq),
                                target: new_id.to_string(),
                                kinds: vec![EventKindFilterWire::Invoke],
                                include_initial: false,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                            if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == folder_id)
                            {
                                p.children.push(new_id);
                            }
                            mounted_objects.push(new_obj);
                            eprintln!(
                                "[desktop-shell] AI create_folder 승인 → {}/{}",
                                folder_path.display(),
                                name
                            );
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI create_folder (응답 후) 실패: {}", e);
                        }
                    }
                    // M11: grant_dir helper로 local + server 동시 동기화.
                    let _ =
                        granted_dirs::grant_dir(granted, stream, &requesting_actor, folder_path)
                            .await;
                }
                dialog_ops::PendingFs::DeleteFile { file_id, path, .. } => {
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(path.clone());
                    }
                    match file_ops::delete_file(&path) {
                        Ok(()) => {
                            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == file_id) {
                                o.state.insert("destroyed".into(), json!(true));
                            }
                            // M10 결함 4 fix: tombstone broadcast — compositor
                            // 트리의 옛 file 객체가 즉시 사라진다.
                            extra_state_sets.push((file_id, "destroyed".to_string(), json!(true)));
                            eprintln!("[desktop-shell] AI delete_file 승인 → {}", path.display());
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI delete_file (응답 후) 실패: {}", e);
                        }
                    }
                    // delete는 grant 안 함 — 다음 delete도 항상 confirm 정책.
                }
                dialog_ops::PendingFs::DeleteFolder { folder_id, path, recursive, .. } => {
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(path.clone());
                    }
                    match folder_ops::delete_folder(&path, recursive) {
                        Ok(()) => {
                            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == folder_id)
                            {
                                o.state.insert("destroyed".into(), json!(true));
                            }
                            // M10 결함 4 fix: tombstone broadcast — 같은 원리.
                            extra_state_sets.push((
                                folder_id,
                                "destroyed".to_string(),
                                json!(true),
                            ));
                            eprintln!("[desktop-shell] AI delete_folder 승인 → {}", path.display());
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI delete_folder (응답 후) 실패: {}", e);
                        }
                    }
                }
                dialog_ops::PendingFs::ExternalWrite { path, content, .. } => {
                    // M10 Phase 3 (ADR-036): cwd *밖* path write.
                    // dir grant 모델 적용 X — 매 호출 confirm 정책 (cwd 밖이라
                    // 항상 위험). Watcher echo도 X — cwd 밖이라 watcher 범위
                    // 밖이고, *_external은 객체 트리에 새 mount도 안 만든다.
                    match std::fs::write(&path, &content) {
                        Ok(()) => {
                            eprintln!(
                                "[desktop-shell] write_external 승인 → {} ({} bytes)",
                                path.display(),
                                content.len()
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "[desktop-shell] write_external (응답 후) 실패 {}: {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
                dialog_ops::PendingFs::Rename {
                    target_id: tid,
                    path,
                    new_name,
                    is_folder,
                    requesting_actor,
                } => {
                    // M10 Phase 2: rename = Remove(old) + Create(new) 두 이벤트.
                    if let Some(w) = fs_watcher {
                        w.mark_self_op(path.clone());
                        if let Some(parent) = path.parent() {
                            w.mark_self_op(parent.join(&new_name));
                        }
                    }
                    let result = if is_folder {
                        folder_ops::rename_folder(&path, &new_name)
                    } else {
                        file_ops::rename_file(&path, &new_name)
                    };
                    match result {
                        Ok(new_path) => {
                            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == tid) {
                                o.props.insert("name".into(), json!(&new_name));
                                o.props.insert("path".into(), json!(new_path.to_string_lossy()));
                            }
                            // M11: grant_dir helper로 local + server 동시 동기화.
                            if let Some(parent) = new_path.parent() {
                                let _ = granted_dirs::grant_dir(
                                    granted,
                                    stream,
                                    &requesting_actor,
                                    parent.to_path_buf(),
                                )
                                .await;
                            }
                            eprintln!("[desktop-shell] AI rename 승인 → {}", new_path.display());
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] AI rename (응답 후) 실패: {}", e);
                        }
                    }
                }
                dialog_ops::PendingFs::ShellRun { cmd, args, cwd, requesting_actor: _ } => {
                    let sr_id = find_shellrunner_id(mounted_objects);
                    let outcome = crate::handlers::shellrunner_methods::execute_command(
                        mounted_objects,
                        sr_id,
                        &cmd,
                        &args,
                        &cwd,
                    )
                    .await;
                    extra_state_sets.extend(outcome.state_sets);
                }
            }
        } else {
            eprintln!("[desktop-shell] AI 요청 거부됨 (action={})", action);
            // ShellRun 거부 시 ShellRunner 객체에 last_error/last_exit_code 반영.
            if let dialog_ops::PendingFs::ShellRun { .. } = &entry.op {
                let sr_id = find_shellrunner_id(mounted_objects);
                if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == sr_id) {
                    o.state.insert("last_error".into(), json!("사용자 거부"));
                    o.state.insert("last_exit_code".into(), json!(-1));
                }
                extra_state_sets.push((sr_id, "last_error".to_string(), json!("사용자 거부")));
                extra_state_sets.push((sr_id, "last_exit_code".to_string(), json!(-1)));
            }
        }
        // 인프라 보존 — tx는 사용 X (동기 처리), 명시적 drop으로 의도 표시.
        drop(entry.tx);
    }
    // Dialog destroy — mounted_objects에서 제거 + SetState destroyed=true.
    // (close 분기와 같은 KI-011 우회 — proto에 DestroyMsg 없음.)
    let dialog_id = target_id;
    mounted_objects.retain(|o| o.id != dialog_id);
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.retain(|c| *c != dialog_id);
    }
    // M10 결함 4 fix: Dialog tombstone + Delete 대상 tombstone을 한꺼번에 broadcast.
    let mut state_sets = extra_state_sets;
    state_sets.push((dialog_id, "destroyed".to_string(), json!(true)));
    Ok(InvokeOutcome { state_sets })
}

/// mounted_objects에서 ShellRunner@1 singleton의 ObjectId를 찾는다.
/// 없으면 nil (ObjectId::nil) — broadcast_error가 아무 객체도 못 찾아 조용히 skip.
fn find_shellrunner_id(mounted_objects: &[geulos_core::Object]) -> geulos_core::ObjectId {
    mounted_objects
        .iter()
        .find(|o| o.type_uri.as_str() == "aios.builtin/ShellRunner@1")
        .map(|o| o.id)
        .unwrap_or_else(geulos_core::ObjectId::nil)
}
