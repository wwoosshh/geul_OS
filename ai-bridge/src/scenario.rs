//! 시나리오 파일 형식 + runner.

use std::path::Path;

use serde::Deserialize;

use crate::error::{BridgeError, BridgeResult};
use crate::session::SessionBudget;

/// TOML 시나리오 파일.
///
/// 형식 예:
/// ```toml
/// name = "explore_system"
/// goal = "Tell me what's on this system."
/// [budget]
/// max_turns = 8
/// max_wall_secs = 60
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub goal: String,
    #[serde(default)]
    pub budget: ScenarioBudget,
}

/// TOML scenario의 budget 섹션.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioBudget {
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default = "default_max_wall")]
    pub max_wall_secs: u64,
    #[serde(default = "default_max_input_tokens")]
    pub max_input_tokens: u64,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u64,
}

impl Default for ScenarioBudget {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            max_wall_secs: default_max_wall(),
            max_input_tokens: default_max_input_tokens(),
            max_output_tokens: default_max_output_tokens(),
        }
    }
}

fn default_max_turns() -> usize {
    12
}
fn default_max_wall() -> u64 {
    120
}
fn default_max_input_tokens() -> u64 {
    200_000
}
fn default_max_output_tokens() -> u64 {
    8_000
}

impl Scenario {
    /// TOML 파일에서 시나리오 로드.
    pub fn load(path: impl AsRef<Path>) -> BridgeResult<Self> {
        let content = std::fs::read_to_string(path).map_err(BridgeError::Io)?;
        let s: Self = toml::from_str(&content)
            .map_err(|e| BridgeError::Config(format!("scenario TOML: {}", e)))?;
        Ok(s)
    }

    /// scenario의 budget을 SessionBudget으로 변환.
    pub fn to_session_budget(&self) -> SessionBudget {
        SessionBudget {
            max_turns: self.budget.max_turns,
            max_wall_secs: self.budget.max_wall_secs,
            max_input_tokens: self.budget.max_input_tokens,
            max_output_tokens: self.budget.max_output_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_scenario() {
        let toml = r#"
name = "test"
goal = "do nothing"
"#;
        let s: Scenario = toml::from_str(toml).unwrap();
        assert_eq!(s.name, "test");
        assert_eq!(s.budget.max_turns, 12); // default
    }

    #[test]
    fn parses_full_scenario() {
        let toml = r#"
name = "press_button"
goal = "Press the button 5 times."
[budget]
max_turns = 20
max_wall_secs = 60
"#;
        let s: Scenario = toml::from_str(toml).unwrap();
        assert_eq!(s.budget.max_turns, 20);
        assert_eq!(s.budget.max_wall_secs, 60);
    }
}
