//! 명령 구현체.
//!
//! 본 모듈은 후속 태스크에서 명령 추가시 점진 확장된다.

use geulos_core::ActorId;

use crate::shell::{Shell, ShellError, ShellOutcome};

/// 명령 한 줄(토큰화된)을 dispatch.
pub fn dispatch(shell: &mut Shell, toks: &[String]) -> Result<ShellOutcome, ShellError> {
    match toks[0].as_str() {
        "help" => help(),
        "exit" | "quit" => Ok(ShellOutcome::Quit),
        "actor" => actor(shell),
        "as" => as_cmd(shell, &toks[1..]),
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
