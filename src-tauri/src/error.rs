//! The one error type. Every command returns `Result<T, TerraceError>`; it serialises as
//! `{ "code": "...", "message": "..." }` so the services layer can branch on the code and
//! show the message.

use serde::Serialize;
use serde::ser::SerializeStruct;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum TerraceError {
    #[error("{0}")]
    Config(String),

    #[error("not connected: {0}")]
    NotConnected(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("kubernetes error: {0}")]
    Kubernetes(String),

    /// The credentials reached the API server and were turned away: not a network problem.
    #[error("not permitted: {0}")]
    Forbidden(String),

    #[error("talos error: {0}")]
    Talos(String),

    #[error("ollama error: {0}")]
    Ollama(String),

    #[error("vault error: {0}")]
    Vault(String),

    #[error("timed out after {0:?}")]
    Timeout(Duration),
}

impl TerraceError {
    /// Stable, screaming-snake code for the front end.
    pub fn code(&self) -> &'static str {
        match self {
            TerraceError::Config(_) => "CONFIG",
            TerraceError::NotConnected(_) => "NOT_CONNECTED",
            TerraceError::Database(_) => "DATABASE",
            TerraceError::Io(_) => "IO",
            TerraceError::Serialization(_) => "SERIALIZATION",
            TerraceError::Kubernetes(_) => "KUBERNETES",
            TerraceError::Forbidden(_) => "FORBIDDEN",
            TerraceError::Talos(_) => "TALOS",
            TerraceError::Ollama(_) => "OLLAMA",
            TerraceError::Vault(_) => "VAULT",
            TerraceError::Timeout(_) => "TIMEOUT",
        }
    }
}

impl Serialize for TerraceError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("TerraceError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<kube::Error> for TerraceError {
    fn from(error: kube::Error) -> Self {
        match error {
            kube::Error::Api(response) if response.code == 401 || response.code == 403 => {
                TerraceError::Forbidden(response.message)
            }
            other => TerraceError::Kubernetes(other.to_string()),
        }
    }
}

impl From<kube::config::KubeconfigError> for TerraceError {
    fn from(error: kube::config::KubeconfigError) -> Self {
        TerraceError::Kubernetes(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TerraceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_403_from_the_api_server_is_forbidden_not_a_network_error() {
        let response = kube::core::Status::failure(
            "nodes is forbidden: User \"reader\" cannot list resource \"nodes\"",
            "Forbidden",
        )
        .with_code(403);
        let error = TerraceError::from(kube::Error::Api(Box::new(response)));
        assert_eq!(error.code(), "FORBIDDEN");
        assert!(error.to_string().contains("cannot list"));
    }

    #[test]
    fn serialises_as_code_and_message() {
        let error = TerraceError::NotConnected("no kubeconfig".into());
        let json = serde_json::to_value(&error).expect("serialise");
        assert_eq!(json["code"], "NOT_CONNECTED");
        assert_eq!(json["message"], "not connected: no kubeconfig");
    }

    #[test]
    fn every_variant_has_a_code() {
        let variants = [
            TerraceError::Config(String::new()),
            TerraceError::Talos(String::new()),
            TerraceError::Timeout(Duration::from_secs(1)),
        ];
        for variant in variants {
            assert!(!variant.code().is_empty());
        }
    }
}
