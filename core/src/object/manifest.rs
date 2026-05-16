//! 앱 매니페스트 (`aios.toml`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::identity::TypeUri;

/// 매니페스트 raw 표현 (toml에서 deserialize).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestRaw {
    id: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    ui_types: Vec<String>,
}

/// 앱 매니페스트.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    pub id: String,
    pub permissions: Vec<String>,
    pub ui_types: Vec<TypeUri>,
}

/// 매니페스트 파싱 오류.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("TOML parse error: {0}")]
    Toml(String),
    #[error("bad TypeUri: {0}")]
    BadTypeUri(String),
}

impl AppManifest {
    pub fn from_toml(s: &str) -> Result<Self, ManifestError> {
        let raw: ManifestRaw = toml::from_str(s).map_err(|e| ManifestError::Toml(e.to_string()))?;
        let mut ui_types = Vec::new();
        for t in raw.ui_types {
            let parsed = TypeUri::parse(&t).map_err(|_| ManifestError::BadTypeUri(t.clone()))?;
            ui_types.push(parsed);
        }
        Ok(Self { id: raw.id, permissions: raw.permissions, ui_types })
    }

    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let raw = ManifestRaw {
            id: self.id.clone(),
            permissions: self.permissions.clone(),
            ui_types: self.ui_types.iter().map(|t| t.as_str().to_string()).collect(),
        };
        toml::to_string(&raw).map_err(|e| ManifestError::Toml(e.to_string()))
    }

    pub fn declares_type(&self, type_uri: &TypeUri) -> bool {
        self.ui_types.iter().any(|t| t == type_uri)
    }
}
