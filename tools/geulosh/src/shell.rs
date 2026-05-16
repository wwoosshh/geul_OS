//! Shell 상태와 명령 디스패치.

use std::collections::HashMap;

use geulos_core::{ActorId, ObjectId, ObjectServer, SubscriptionId};

use crate::commands;
use crate::parser::tokenize;

/// 셸 명령 한 줄의 실행 결과.
#[derive(Debug, Clone)]
pub enum ShellOutcome {
    /// 정상 출력.
    Output(String),
    /// 에러 메시지.
    Error(String),
    /// 종료 요청 (exit/quit).
    Quit,
    /// 출력 없음 (빈 줄, 주석 등).
    NoOp,
}

/// 셸 에러.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// 알 수 없는 명령.
    #[error("unknown command: '{0}' — type `help`")]
    UnknownCommand(String),
    /// 인자 부족.
    #[error("usage: {0}")]
    Usage(String),
    /// 라벨 미정의.
    #[error("no such label: {0} — try `ls`")]
    BadLabel(String),
    /// core 에러 위임.
    #[error("{0}")]
    Core(String),
}

/// 셸 상태.
#[allow(dead_code)]
pub struct Shell {
    /// 핵심: 객체 서버.
    pub(crate) server: ObjectServer,
    /// 현재 액터.
    pub(crate) current_actor: ActorId,
    /// 한 번 발급된 default AI 액터 (sticky).
    pub(crate) default_ai: Option<ActorId>,
    /// 짧은 라벨 (`#N`) → ObjectId.
    pub(crate) labels: HashMap<u32, ObjectId>,
    /// 다음 라벨 번호.
    pub(crate) next_label: u32,
    /// 구독 라벨 (`@N`) → SubscriptionId.
    pub(crate) sub_labels: HashMap<u32, SubscriptionId>,
    /// 다음 구독 라벨 번호.
    pub(crate) next_sub_label: u32,
}

#[allow(dead_code)]
impl Shell {
    /// 빈 셸.
    pub fn new() -> Self {
        Self {
            server: ObjectServer::new(),
            current_actor: ActorId::local_user(),
            default_ai: None,
            labels: HashMap::new(),
            next_label: 1,
            sub_labels: HashMap::new(),
            next_sub_label: 1,
        }
    }

    /// `#N` 라벨 또는 UUID 문자열을 ObjectId로 해석.
    pub(crate) fn resolve_object(&self, token: &str) -> Result<ObjectId, ShellError> {
        if let Some(n_str) = token.strip_prefix('#') {
            let n: u32 = n_str.parse().map_err(|_| ShellError::BadLabel(token.to_string()))?;
            self.labels.get(&n).copied().ok_or_else(|| ShellError::BadLabel(token.to_string()))
        } else {
            Err(ShellError::BadLabel(token.to_string()))
        }
    }

    /// 새 짧은 라벨 부여.
    pub(crate) fn assign_label(&mut self, id: ObjectId) -> u32 {
        let n = self.next_label;
        self.labels.insert(n, id);
        self.next_label += 1;
        n
    }

    /// 한 줄 명령 실행.
    pub fn execute(&mut self, line: &str) -> ShellOutcome {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return ShellOutcome::NoOp;
        }
        let toks = tokenize(trimmed);
        if toks.is_empty() {
            return ShellOutcome::NoOp;
        }
        match commands::dispatch(self, &toks) {
            Ok(out) => out,
            Err(e) => ShellOutcome::Error(e.to_string()),
        }
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
