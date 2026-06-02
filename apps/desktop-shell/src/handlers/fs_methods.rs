//! File/Folder fs 변경 method handler — save_to_file/save/create_file/create_folder/
//! delete/rename/read/list (M9 T8, M10 T7).
//!
//! 각 handler는 permission::judge_with_path로 dir grant 판정 → Allow면 즉시 fs op +
//! mount/subscribe/parent.children 갱신, ConfirmRequired면 Dialog mount + PendingMap에
//! 해당 PendingFs variant 보관. respond 분기 (dialog_methods)가 take → 실제 실행.
//!
//! save_to_file은 *컴포지터 Ctrl+S* 전용 — 권한 검사 없이 항상 허용 (사용자 직접 액션).
//! save는 *AI/외부 actor*의 File.save invoke — permission 검사 후 분기.

use std::path::PathBuf;

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingMap};
use crate::fs_watcher::FsWatcher;
use crate::granted_dirs::GrantedDirs;
use crate::handlers::{add_dialog_acl, add_fs_object_acl, lazy_expand_if_needed, lookup_file_path};
use crate::invoke_handler::InvokeOutcome;
use crate::{file_ops, file_write, folder_ops, permission};

/// Window.save_to_file(content) — 사용자 Ctrl+S. compositor가 *editor local content*
/// 를 args.content로 실어 보냄 (Window.state.content는 *읽지 않음*).
///
/// 이유 (사용자 보고 freeze fix): 이전 v1은 매 키 입력마다 SetState(content)를 wire에
/// push해서 큰 텍스트 파일에서 wire backpressure로 입력 freeze 발생. 이제 content는
/// 컴포지터가 master, save 시점에만 args로 한 번 전달. desktop-shell이 args.content를
/// 직접 디스크에 commit + Window.state.content도 같이 갱신해서 다음 viewer load 일관.
///
/// 사용자 직접 액션이므로 permission::judge(local-user, Save) = Allow.
pub fn handle_save_to_file(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    eprintln!(
        "[desktop-shell] save_to_file invoke 수신 target={} content_len={}",
        target_id,
        content.len()
    );
    // Window.save_to_file은 *compositor의 Ctrl+S가 발송*하는 UI 직접 액션이므로
    // 권한 검사 없이 항상 허용. AI 등 외부 actor가 File 자체에 write할 때만
    // (`File.save` invoke) permission::judge로 Dialog confirm을 띄운다.
    let file_id_opt = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|w| w.props.get("file_id").and_then(|v| v.as_str()))
        .and_then(crate::handlers::parse_object_id);
    let file_id = match file_id_opt {
        Some(id) => id,
        None => {
            eprintln!(
                "[desktop-shell] save_to_file: Window.props.file_id 누락 또는 파싱 실패 (target={})",
                target_id
            );
            return InvokeOutcome::empty();
        }
    };
    let path = match lookup_file_path(mounted_objects, file_id) {
        Some(p) => p,
        None => {
            eprintln!("[desktop-shell] save_to_file: file_id={}의 path 조회 실패", file_id);
            return InvokeOutcome::empty();
        }
    };
    match file_write::save(&path, &content) {
        Ok(()) => {
            eprintln!("[desktop-shell] save_to_file OK → {}", path.display());
            if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                w.state.insert("dirty".into(), json!(false));
                w.state.insert("content".into(), json!(&content));
            }
            InvokeOutcome {
                state_sets: vec![
                    (target_id, "dirty".to_string(), json!(false)),
                    (target_id, "content".to_string(), json!(content)),
                ],
            }
        }
        Err(e) => {
            eprintln!("[desktop-shell] save_to_file 실패: {}", e);
            InvokeOutcome::empty()
        }
    }
}

/// File.save(content) — AI/외부 actor가 직접 호출. sender_actor가 local-user면 Allow,
/// AI면 ConfirmRequired → Dialog + PendingMap.insert.
#[allow(clippy::too_many_arguments)]
pub async fn handle_save(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    granted: &GrantedDirs,
    pending: &PendingMap,
    fs_watcher: Option<&FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let p = match lookup_file_path(mounted_objects, target_id) {
        Some(p) => p,
        None => return Ok(InvokeOutcome::empty()),
    };
    // M10 결함 3 fix: path-aware judge — 파일의 parent dir grant가 있으면
    // 사용자 confirm 없이 즉시 save. M9는 path-blind라 AI가 같은 dir에
    // create_file을 grant 받았어도 그 dir 안 *기존 파일* save는 매번
    // Dialog → UX 회귀. dir 단위 grant 모델 (ADR-036)과 일관.
    // 호스트 경로(D:\) 인지 parent_of — std::path::parent()는 VM(Linux)에서 D:\a\b의 부모를
    // 빈 값으로 반환해 워크스페이스 grant가 호스트 파일 save에 안 먹던 버그.
    let save_dir = crate::granted_dirs::parent_of(&p).unwrap_or_default();
    let verdict =
        permission::judge_with_path(sender_actor, permission::Op::Save, &save_dir, granted);
    match verdict {
        permission::Verdict::Allow => {
            // M10 Phase 2: echo — save 직후 watcher가 Modified를 보고함.
            if let Some(w) = fs_watcher {
                w.mark_self_op(p.clone());
            }
            match file_write::save(&p, &content) {
                Ok(()) => Ok(InvokeOutcome {
                    state_sets: vec![(target_id, "dirty".to_string(), json!(false))],
                }),
                Err(e) => {
                    eprintln!("[desktop-shell] save 실패: {}", e);
                    Ok(InvokeOutcome::empty())
                }
            }
        }
        permission::Verdict::ConfirmRequired => {
            // Dialog mount — desktop 자식, modal.
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 저장 확인",
                &format!("AI가 {}를 저장하려고 합니다 — 허용?", p.display()),
                "confirm",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;

            // wire 송신 — MountMsg + Invoke SubscribeMsg.
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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

            // PendingMap에 보관 — respond 분기가 take + save 실행.
            // oneshot tx는 v1에서 사용 X (동기 처리). 인프라 보존.
            let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
            pending.insert(
                dialog_id,
                dialog_ops::PendingEntry {
                    op: dialog_ops::PendingFs::Save {
                        file_id: target_id,
                        path: p.clone(),
                        content,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI save Dialog mount (file {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
    }
}

/// Folder.create_file(name) — 폴더 안에 새 빈 파일 생성. dir grant 판정 → Allow면 즉시
/// fs + mount/subscribe/parent.children, ConfirmRequired면 Dialog + PendingFs::CreateFile.
#[allow(clippy::too_many_arguments)]
pub async fn handle_create_file(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    granted: &GrantedDirs,
    pending: &PendingMap,
    fs_watcher: Option<&FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let folder_path = match mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|f| f.props.get("path").and_then(|v| v.as_str()))
        .map(PathBuf::from)
    {
        Some(p) => p,
        None => {
            eprintln!("[desktop-shell] create_file: folder path 누락 target={}", target_id);
            return Ok(InvokeOutcome::empty());
        }
    };
    let verdict = permission::judge_with_path(
        sender_actor,
        permission::Op::CreateFile,
        &folder_path,
        granted,
    );
    match verdict {
        permission::Verdict::Allow => {
            let now = chrono::Utc::now().timestamp_millis();
            // M10 Phase 2: 우리가 막 만들 파일을 watcher echo 캐시에
            // 미리 등록 — fs::write 직후 도착할 notify 이벤트는 무시.
            if let Some(w) = fs_watcher {
                w.mark_self_op(folder_path.join(&name));
            }
            match folder_ops::create_file_in(owner, &folder_path, &name, now) {
                Ok(mut new_obj) => {
                    new_obj.parent = Some(target_id);
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
                    if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        p.children.push(new_id);
                    }
                    mounted_objects.push(new_obj);
                    eprintln!(
                        "[desktop-shell] create_file OK → {}/{}",
                        folder_path.display(),
                        name
                    );
                    Ok(InvokeOutcome::empty())
                }
                Err(e) => {
                    eprintln!("[desktop-shell] create_file 실패: {}", e);
                    Ok(InvokeOutcome::empty())
                }
            }
        }
        permission::Verdict::ConfirmRequired => {
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 파일 생성 확인",
                &format!(
                    "AI가 {} 안에 '{}'를 생성하려고 합니다 — 허용?",
                    folder_path.display(),
                    name
                ),
                "confirm",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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
                    op: dialog_ops::PendingFs::CreateFile {
                        folder_id: target_id,
                        folder_path,
                        name,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI create_file Dialog mount (folder {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
    }
}

/// Folder.create_folder(name) — create_file과 동일 패턴, fs는 create_dir, Dialog/Pending은
/// CreateFolder variant.
#[allow(clippy::too_many_arguments)]
pub async fn handle_create_folder(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    granted: &GrantedDirs,
    pending: &PendingMap,
    fs_watcher: Option<&FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let folder_path = match mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|f| f.props.get("path").and_then(|v| v.as_str()))
        .map(PathBuf::from)
    {
        Some(p) => p,
        None => {
            eprintln!("[desktop-shell] create_folder: folder path 누락 target={}", target_id);
            return Ok(InvokeOutcome::empty());
        }
    };
    let verdict = permission::judge_with_path(
        sender_actor,
        permission::Op::CreateFolder,
        &folder_path,
        granted,
    );
    match verdict {
        permission::Verdict::Allow => {
            let now = chrono::Utc::now().timestamp_millis();
            // M10 Phase 2: echo 캐시 — 새로 만들 폴더 path 등록.
            if let Some(w) = fs_watcher {
                w.mark_self_op(folder_path.join(&name));
            }
            match folder_ops::create_folder_in(owner, &folder_path, &name, now) {
                Ok(mut new_obj) => {
                    new_obj.parent = Some(target_id);
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
                    if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        p.children.push(new_id);
                    }
                    mounted_objects.push(new_obj);
                    eprintln!(
                        "[desktop-shell] create_folder OK → {}/{}",
                        folder_path.display(),
                        name
                    );
                    Ok(InvokeOutcome::empty())
                }
                Err(e) => {
                    eprintln!("[desktop-shell] create_folder 실패: {}", e);
                    Ok(InvokeOutcome::empty())
                }
            }
        }
        permission::Verdict::ConfirmRequired => {
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 폴더 생성 확인",
                &format!(
                    "AI가 {} 안에 '{}' 폴더를 생성하려고 합니다 — 허용?",
                    folder_path.display(),
                    name
                ),
                "confirm",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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
                    op: dialog_ops::PendingFs::CreateFolder {
                        folder_id: target_id,
                        folder_path,
                        name,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI create_folder Dialog mount (folder {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
    }
}

/// File.delete or Folder.delete(recursive). target type_uri로 분기. Delete는 *항상
/// ConfirmRequired* (granted 무관 — permission 정책 보장). Dialog kind="warn".
#[allow(clippy::too_many_arguments)]
pub async fn handle_delete(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let target_obj_kind =
        mounted_objects.iter().find(|o| o.id == target_id).map(|o| o.type_uri.as_str().to_string());
    let path_opt = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
        .map(PathBuf::from);
    let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
    match (target_obj_kind.as_deref(), path_opt) {
        (Some("aios.std/File@1"), Some(path)) => {
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 파일 삭제 확인",
                &format!("AI가 {}를 삭제하려고 합니다 — 허용?", path.display()),
                "warn",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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
                    op: dialog_ops::PendingFs::DeleteFile {
                        file_id: target_id,
                        path,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI delete_file Dialog mount (file {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
        (Some("aios.std/Folder@1"), Some(path)) => {
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 폴더 삭제 확인",
                &format!(
                    "AI가 {}를 {}삭제하려고 합니다 — 허용?",
                    path.display(),
                    if recursive { "재귀 " } else { "" }
                ),
                "warn",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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
                    op: dialog_ops::PendingFs::DeleteFolder {
                        folder_id: target_id,
                        path,
                        recursive,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI delete_folder Dialog mount (folder {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
        _ => {
            eprintln!("[desktop-shell] delete: unknown type 또는 path 누락 target={}", target_id);
            Ok(InvokeOutcome::empty())
        }
    }
}

/// File.rename or Folder.rename(new_name). target type 판정 → parent_dir에 대한
/// permission::judge_with_path(Rename) → Allow면 즉시 fs::rename + props 갱신,
/// ConfirmRequired면 Dialog + Pending::Rename. respond 분기가 take + grant 추가.
#[allow(clippy::too_many_arguments)]
pub async fn handle_rename(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    granted: &GrantedDirs,
    pending: &PendingMap,
    fs_watcher: Option<&FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let new_name = args.get("new_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let target_obj = mounted_objects.iter().find(|o| o.id == target_id);
    let target_obj_kind = target_obj.map(|o| o.type_uri.as_str().to_string());
    let path_opt =
        target_obj.and_then(|o| o.props.get("path").and_then(|v| v.as_str())).map(PathBuf::from);
    let is_folder = matches!(target_obj_kind.as_deref(), Some("aios.std/Folder@1"));
    let path = match (target_obj_kind.as_deref(), path_opt) {
        (Some("aios.std/File@1"), Some(p)) | (Some("aios.std/Folder@1"), Some(p)) => p,
        _ => {
            eprintln!("[desktop-shell] rename: unknown type 또는 path 누락 target={}", target_id);
            return Ok(InvokeOutcome::empty());
        }
    };
    // 호스트 경로 인지 parent_of (std parent()는 VM에서 D:\ 부모를 빈 값으로 반환).
    let parent_dir = crate::granted_dirs::parent_of(&path).unwrap_or_else(|| PathBuf::from("/"));
    let verdict =
        permission::judge_with_path(sender_actor, permission::Op::Rename, &parent_dir, granted);
    match verdict {
        permission::Verdict::Allow => {
            // M10 Phase 2: echo — rename은 old path Remove + new path
            // Create 두 이벤트가 발생. 둘 다 mark.
            if let Some(w) = fs_watcher {
                w.mark_self_op(path.clone());
                w.mark_self_op(parent_dir.join(&new_name));
            }
            let result = if is_folder {
                folder_ops::rename_folder(&path, &new_name)
            } else {
                file_ops::rename_file(&path, &new_name)
            };
            match result {
                Ok(new_path) => {
                    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        o.props.insert("name".into(), json!(&new_name));
                        o.props.insert("path".into(), json!(new_path.to_string_lossy()));
                    }
                    eprintln!("[desktop-shell] rename OK → {}", new_path.display());
                    Ok(InvokeOutcome {
                        state_sets: vec![(target_id, "name".to_string(), json!(&new_name))],
                    })
                }
                Err(e) => {
                    eprintln!("[desktop-shell] rename 실패: {}", e);
                    Ok(InvokeOutcome::empty())
                }
            }
        }
        permission::Verdict::ConfirmRequired => {
            let mut dialog = std_types::dialog(
                owner.clone(),
                "AI 이름 변경 확인",
                &format!(
                    "AI가 {}를 '{}'(으)로 이름 변경하려고 합니다 — 허용?",
                    path.display(),
                    new_name
                ),
                "confirm",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_dialog_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
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
                    op: dialog_ops::PendingFs::Rename {
                        target_id,
                        path,
                        new_name,
                        is_folder,
                        requesting_actor: sender_actor.clone(),
                    },
                    tx,
                },
            );
            eprintln!(
                "[desktop-shell] AI rename Dialog mount (target {}): 사용자 응답 대기",
                target_id
            );
            Ok(InvokeOutcome::empty())
        }
    }
}

/// File.read — AI가 *fresh content + size*를 동적으로 조회. lazy_mount 시점
/// 의 stale state 대신 fs::read를 새로 호출해 SetState로 broadcast. AI는
/// invoke 후 subscribe + drain 또는 get_object로 fresh state 인지.
pub fn handle_read(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let path_opt = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .filter(|o| o.type_uri.as_str() == "aios.std/File@1")
        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
        .map(std::path::PathBuf::from);
    let path = match path_opt {
        Some(p) => p,
        None => return InvokeOutcome::empty(),
    };

    // 호스트 경로(C:\... 등)면 host bridge 통해 읽음. VM(non-windows) 빌드에서만.
    // VM(Linux)이 직접 Windows path를 fs::read하면 ENOENT — AI tool 호출이 침묵 실패.
    #[cfg(not(windows))]
    let read_result: Result<String, String> = {
        let path_str = path.to_string_lossy().to_string();
        if crate::host_bridge_client::is_host_path(&path_str) {
            const MAX: u64 = 1 << 20; // 1MB cap (read_file_for_window와 동일)
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
            let size = content.len() as i64;
            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                o.state.insert("content".into(), json!(&content));
                o.state.insert("size".into(), json!(size));
            }
            eprintln!("[desktop-shell] File.read OK ({} bytes) → {}", size, path.display());
            InvokeOutcome {
                state_sets: vec![
                    (target_id, "content".to_string(), json!(content)),
                    (target_id, "size".to_string(), json!(size)),
                ],
            }
        }
        Err(e) => {
            eprintln!("[desktop-shell] File.read 실패 {}: {}", path.display(), e);
            InvokeOutcome::empty()
        }
    }
}

/// Folder.list — AI가 *expand되지 않은* 폴더의 children을 동적으로 mount + 인지.
/// 사용자가 FileTree로 안 열어둬도 AI는 list 호출로 즉시 자식 트리 접근.
#[allow(clippy::too_many_arguments)]
pub async fn handle_list(
    target_id: ObjectId,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    fs_watcher: Option<&mut FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    if !mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .map(|o| o.type_uri.as_str() == "aios.std/Folder@1")
        .unwrap_or(false)
    {
        return Ok(InvokeOutcome::empty());
    }
    // 기존 lazy_expand 흐름 재사용 — 직계 children mount + subscribe.
    lazy_expand_if_needed(stream, mounted_objects, owner, target_id, req_seq, fs_watcher).await?;
    let count =
        mounted_objects.iter().find(|o| o.id == target_id).map(|o| o.children.len()).unwrap_or(0);
    eprintln!("[desktop-shell] Folder.list → {} children", count);
    Ok(InvokeOutcome { state_sets: vec![(target_id, "child_count".to_string(), json!(count))] })
}
