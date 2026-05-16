//! 메서드 시그니처 정의.

use serde::{Deserialize, Serialize};

/// 객체 메서드의 인자 사양.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    name: String,
    type_hint: String,
}

impl ArgSpec {
    /// 새 ArgSpec.
    pub fn new(name: impl Into<String>, type_hint: impl Into<String>) -> Self {
        Self { name: name.into(), type_hint: type_hint.into() }
    }

    /// 인자 이름.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 타입 힌트 (예: "integer", "string").
    pub fn type_hint(&self) -> &str {
        &self.type_hint
    }
}

/// 객체가 제공하는 메서드의 시그니처.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodSig {
    name: String,
    args: Vec<ArgSpec>,
    returns: Option<String>,
}

impl MethodSig {
    /// 새 MethodSig.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), args: Vec::new(), returns: None }
    }

    /// 인자 추가 (체이닝).
    pub fn with_arg(mut self, arg: ArgSpec) -> Self {
        self.args.push(arg);
        self
    }

    /// 반환 타입 설정 (체이닝).
    pub fn with_returns(mut self, type_hint: impl Into<String>) -> Self {
        self.returns = Some(type_hint.into());
        self
    }

    /// 메서드 이름.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 인자 목록.
    pub fn args(&self) -> &[ArgSpec] {
        &self.args
    }

    /// 반환 타입 힌트.
    pub fn returns(&self) -> Option<&str> {
        self.returns.as_deref()
    }
}
