//! 명령 구현체.
//!
//! 본 모듈은 후속 태스크에서 명령 추가시 점진 확장된다.

use geulos_core::{std_types, ActorId, Query, TypeUri};
use serde_json::Value;

use crate::output::{event_short, object_detail, one_line};
use crate::shell::{Shell, ShellError, ShellOutcome};

/// 명령 한 줄(토큰화된)을 dispatch.
pub fn dispatch(shell: &mut Shell, toks: &[String]) -> Result<ShellOutcome, ShellError> {
    match toks[0].as_str() {
        "help" => help(),
        "exit" | "quit" => Ok(ShellOutcome::Quit),
        "actor" => actor(shell),
        "as" => as_cmd(shell, &toks[1..]),
        "mount" => mount(shell, &toks[1..]),
        "ls" => ls(shell),
        "tree" => tree(shell),
        "get" => get(shell, &toks[1..]),
        "events" => events(shell, &toks[1..]),
        "invoke" => invoke(shell, &toks[1..]),
        "query" => query(shell, &toks[1..]),
        cmd => Err(ShellError::UnknownCommand(cmd.to_string())),
    }
}

fn help() -> Result<ShellOutcome, ShellError> {
    let text = "\
GeulOS shell commands:
  help                          이 도움말
  exit | quit                   셸 종료
  actor                         현재 액터 ID
  as user|ai|system             액터 전환
  mount container               (Task 3에서 구현)
  mount text \"내용\"             (Task 3)
  mount button \"label\"          (Task 3)
  mount toggle on|off           (Task 3)
  ls / tree / get #N            (Task 4)
  events [N]                    (Task 4)
  invoke #N <method> [args]     (Task 5)
  query type|owner <value>      (Task 5)
  subscribe #N <filter>...      (Task 6)
  drain @N / unsubscribe @N     (Task 6)
";
    Ok(ShellOutcome::Output(text.trim_end().to_string()))
}

fn actor(shell: &Shell) -> Result<ShellOutcome, ShellError> {
    Ok(ShellOutcome::Output(shell.current_actor.as_str().to_string()))
}

fn as_cmd(shell: &mut Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let kind = args.first().ok_or_else(|| ShellError::Usage("as user|ai|system".to_string()))?;
    match kind.as_str() {
        "user" => {
            shell.current_actor = ActorId::local_user();
            Ok(ShellOutcome::Output(format!("now: {}", shell.current_actor.as_str())))
        }
        "ai" => {
            if shell.default_ai.is_none() {
                shell.default_ai = Some(ActorId::new_ai_session());
            }
            shell.current_actor = shell.default_ai.clone().unwrap();
            Ok(ShellOutcome::Output(format!("now: {}", shell.current_actor.as_str())))
        }
        "system" => {
            shell.current_actor = ActorId::system_compositor();
            Ok(ShellOutcome::Output(format!("now: {}", shell.current_actor.as_str())))
        }
        other => {
            Err(ShellError::Usage(format!("unknown actor kind: '{}' — use user|ai|system", other)))
        }
    }
}

fn mount(shell: &mut Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let kind = args
        .first()
        .ok_or_else(|| ShellError::Usage("mount container|text|button|toggle".to_string()))?;

    let obj = match kind.as_str() {
        "container" => std_types::container(shell.current_actor.clone()),
        "text" => {
            let content = args
                .get(1)
                .ok_or_else(|| ShellError::Usage(r#"mount text "content""#.to_string()))?;
            std_types::text(shell.current_actor.clone(), content)
        }
        "button" => {
            let label = args
                .get(1)
                .ok_or_else(|| ShellError::Usage(r#"mount button "label""#.to_string()))?;
            std_types::button(shell.current_actor.clone(), label)
        }
        "toggle" => {
            let state =
                args.get(1).ok_or_else(|| ShellError::Usage("mount toggle on|off".to_string()))?;
            let on = match state.as_str() {
                "on" => true,
                "off" => false,
                _ => return Err(ShellError::Usage("mount toggle on|off".to_string())),
            };
            std_types::toggle(shell.current_actor.clone(), on)
        }
        other => return Err(ShellError::Usage(format!("unknown mount kind: '{}'", other))),
    };

    let type_uri = obj.type_uri.as_str().to_string();
    let id = shell.server.mount(obj).map_err(|e| ShellError::Core(e.to_string()))?;
    let label = shell.assign_label(id);

    Ok(ShellOutcome::Output(format!("Created #{} ({})", label, type_uri)))
}

fn ls(shell: &Shell) -> Result<ShellOutcome, ShellError> {
    let mut entries: Vec<(u32, _)> = shell.labels.iter().map(|(n, id)| (*n, *id)).collect();
    entries.sort_by_key(|(n, _)| *n);

    if entries.is_empty() {
        return Ok(ShellOutcome::Output("(no objects)".to_string()));
    }

    let mut lines = Vec::new();
    for (n, id) in entries {
        if let Some(obj) = shell.server.get(&id) {
            lines.push(one_line(n, obj));
        }
    }
    Ok(ShellOutcome::Output(lines.join("\n")))
}

fn tree(shell: &Shell) -> Result<ShellOutcome, ShellError> {
    if shell.server.roots().is_empty() {
        return Ok(ShellOutcome::Output("(empty tree)".to_string()));
    }
    let mut lines = Vec::new();
    for root_id in shell.server.roots() {
        let label =
            shell.labels.iter().find(|(_, id)| *id == root_id).map(|(n, _)| *n).unwrap_or(0);
        if let Some(obj) = shell.server.get(root_id) {
            lines.push(format!("- {}", one_line(label, obj)));
            for child_id in &obj.children {
                let child_label = shell
                    .labels
                    .iter()
                    .find(|(_, id)| *id == child_id)
                    .map(|(n, _)| *n)
                    .unwrap_or(0);
                if let Some(child) = shell.server.get(child_id) {
                    lines.push(format!("    \u{2514}\u{2500} {}", one_line(child_label, child)));
                }
            }
        }
    }
    Ok(ShellOutcome::Output(lines.join("\n")))
}

fn get(shell: &Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let target = args.first().ok_or_else(|| ShellError::Usage("get #N".to_string()))?;
    let id = shell.resolve_object(target)?;
    let obj =
        shell.server.get(&id).ok_or_else(|| ShellError::Core("object disappeared".to_string()))?;
    Ok(ShellOutcome::Output(object_detail(obj)))
}

fn events(shell: &Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let n: usize = match args.first() {
        Some(s) => s.parse().map_err(|_| ShellError::Usage("events [N]".to_string()))?,
        None => 10,
    };
    let log = shell.server.bus().log();
    let start = log.len().saturating_sub(n);
    let recent = &log[start..];
    if recent.is_empty() {
        return Ok(ShellOutcome::Output("(no events)".to_string()));
    }
    let lines: Vec<String> = recent.iter().map(event_short).collect();
    Ok(ShellOutcome::Output(lines.join("\n")))
}

fn invoke(shell: &mut Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let target_tok =
        args.first().ok_or_else(|| ShellError::Usage("invoke #N <method> [args]".to_string()))?;
    let method =
        args.get(1).ok_or_else(|| ShellError::Usage("invoke #N <method> [args]".to_string()))?;
    let id = shell.resolve_object(target_tok)?;

    let parsed_args: Value = if args.len() > 2 {
        let joined = args[2..].join(" ");
        serde_json::from_str(&joined).unwrap_or(Value::String(joined))
    } else {
        Value::Null
    };

    let actor = shell.current_actor.clone();
    let event_id = shell
        .server
        .invoke(&actor, &id, method, parsed_args)
        .map_err(|e| ShellError::Core(e.to_string()))?;
    Ok(ShellOutcome::Output(format!("Invoke event {} emitted", event_id)))
}

fn query(shell: &Shell, args: &[String]) -> Result<ShellOutcome, ShellError> {
    let kind =
        args.first().ok_or_else(|| ShellError::Usage("query type|owner <value>".to_string()))?;
    let value =
        args.get(1).ok_or_else(|| ShellError::Usage("query type|owner <value>".to_string()))?;
    let q = match kind.as_str() {
        "type" => {
            let t = TypeUri::parse(value).map_err(|e| ShellError::Core(e.to_string()))?;
            Query::by_type(t)
        }
        "owner" => Query::by_owner(parse_actor_for_query(value)),
        other => return Err(ShellError::Usage(format!("unknown query kind: '{}'", other))),
    };
    let ids = shell.server.query(&q);
    if ids.is_empty() {
        return Ok(ShellOutcome::Output("(no match)".to_string()));
    }
    let mut lines = Vec::new();
    for id in ids {
        let label = shell.labels.iter().find(|(_, oid)| **oid == id).map(|(n, _)| *n).unwrap_or(0);
        if let Some(obj) = shell.server.get(&id) {
            lines.push(one_line(label, obj));
        }
    }
    Ok(ShellOutcome::Output(lines.join("\n")))
}

/// `query owner <token>` 에서 문자열을 ActorId로 변환.
///
/// ActorId 외부 생성자가 없으므로, 알려진 prefix별로 분기한다.
/// `user:local`과 `system:compositor`만 정확 매칭 가능.
/// `ai:<uuid>` 및 `app:<id>:<uuid>` 매칭은 향후 `ActorId::from_raw` API 추가 후 지원.
fn parse_actor_for_query(s: &str) -> ActorId {
    if s == "user:local" {
        ActorId::local_user()
    } else if s == "system:compositor" {
        ActorId::system_compositor()
    } else {
        // ai:<uuid> 또는 app:<id>:<uuid> — 정확 매칭 불가.
        // fallback: local_user()는 비교 시 false가 되어 결과 0개 반환.
        ActorId::local_user()
    }
}
