/// Unified error types for the Coast project.
///
/// Uses `thiserror` for structured error variants. Each variant provides
/// actionable context about what went wrong and how to fix it.
use std::path::PathBuf;

/// The primary error type used across all Coast library crates.
#[derive(Debug, thiserror::Error)]
pub enum CoastError {
    /// Error parsing or validating a Coastfile.
    #[error("Coastfile error: {message}")]
    CoastfileParse {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error interacting with Docker (host or inner daemon).
    #[error("Docker error: {message}")]
    Docker {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with git operations (branch checkout, archive).
    #[error("Git error: {message}")]
    Git {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with secret extraction, encryption, or injection.
    #[error("Secret error: {message}")]
    Secret {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with the state database (SQLite).
    #[error("State error: {message}")]
    State {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with port allocation or forwarding.
    #[error("Port error: {message}")]
    Port {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// General I/O error.
    #[error("I/O error at {path}: {message}")]
    Io {
        message: String,
        path: PathBuf,
        #[source]
        source: Option<std::io::Error>,
    },

    /// Error building the coast image artifact.
    #[error("Artifact error: {message}")]
    Artifact {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with volume operations.
    #[error("Volume error: {message}")]
    Volume {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Instance not found.
    #[error("Instance '{name}' not found in project '{project}'. Run `coast ls` to see available instances.")]
    InstanceNotFound { name: String, project: String },

    /// Instance already exists.
    #[error("Instance '{name}' already exists in project '{project}'. Run `coast rm {name}` first or choose a different name.")]
    InstanceAlreadyExists { name: String, project: String },

    /// A Docker container with the expected name exists but the state DB has no record of it.
    #[error(
        "A dangling Docker container '{container_name}' was found for instance '{name}' \
         in project '{project}', but no matching instance exists in the Coast database. \
         This is likely left over from a previous failed run or interrupted removal.\n\
         To remove it and proceed, re-run with --force-remove-dangling:\n  \
         coast run {name} --force-remove-dangling"
    )]
    DanglingContainerDetected {
        name: String,
        project: String,
        container_name: String,
    },

    /// Runtime not available.
    #[error("Runtime '{runtime}' is not available: {reason}")]
    RuntimeUnavailable { runtime: String, reason: String },

    /// Protocol/IPC error.
    #[error("Protocol error: {message}")]
    Protocol {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error with remote VM operations (SSH, tunnels, sync).
    #[error("Remote error: {message}")]
    Remote { message: String },
}

/// Convenience alias for Results using CoastError.
pub type Result<T> = std::result::Result<T, CoastError>;

impl CoastError {
    /// Create a Coastfile parse error with a message.
    pub fn coastfile(msg: impl Into<String>) -> Self {
        Self::CoastfileParse {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a Docker error with a message.
    pub fn docker(msg: impl Into<String>) -> Self {
        Self::Docker {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a Git error with a message.
    pub fn git(msg: impl Into<String>) -> Self {
        Self::Git {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a Secret error with a message.
    pub fn secret(msg: impl Into<String>) -> Self {
        Self::Secret {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a State error with a message.
    pub fn state(msg: impl Into<String>) -> Self {
        Self::State {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a Port error with a message.
    pub fn port(msg: impl Into<String>) -> Self {
        Self::Port {
            message: msg.into(),
            source: None,
        }
    }

    /// Create an I/O error with path context.
    pub fn io(msg: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::Io {
            message: msg.into(),
            path: path.into(),
            source: None,
        }
    }

    /// Create an Artifact error with a message.
    pub fn artifact(msg: impl Into<String>) -> Self {
        Self::Artifact {
            message: msg.into(),
            source: None,
        }
    }

    /// Create a Protocol error with a message.
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol {
            message: msg.into(),
            source: None,
        }
    }

    /// Create an I/O error without a specific path.
    pub fn io_simple(msg: impl Into<String>) -> Self {
        Self::Io {
            message: msg.into(),
            path: PathBuf::from("<none>"),
            source: None,
        }
    }
}

impl From<std::io::Error> for CoastError {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            message: err.to_string(),
            path: PathBuf::from("<unknown>"),
            source: Some(err),
        }
    }
}

impl From<serde_json::Error> for CoastError {
    fn from(err: serde_json::Error) -> Self {
        Self::Protocol {
            message: format!("JSON serialization error: {err}"),
            source: Some(Box::new(err)),
        }
    }
}

impl From<toml::de::Error> for CoastError {
    fn from(err: toml::de::Error) -> Self {
        Self::CoastfileParse {
            message: format!("TOML parse error: {err}"),
            source: Some(Box::new(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coastfile_error_display() {
        let err = CoastError::coastfile("missing required field 'name'");
        assert_eq!(
            err.to_string(),
            "Coastfile error: missing required field 'name'"
        );
    }

    #[test]
    fn test_instance_not_found_display() {
        let err = CoastError::InstanceNotFound {
            name: "feature-x".to_string(),
            project: "my-app".to_string(),
        };
        assert!(err.to_string().contains("feature-x"));
        assert!(err.to_string().contains("coast ls"));
    }

    #[test]
    fn test_instance_already_exists_display() {
        let err = CoastError::InstanceAlreadyExists {
            name: "feature-x".to_string(),
            project: "my-app".to_string(),
        };
        assert!(err.to_string().contains("coast rm feature-x"));
    }

    #[test]
    fn test_runtime_unavailable_display() {
        let err = CoastError::RuntimeUnavailable {
            runtime: "sysbox".to_string(),
            reason: "sysbox-runc not found".to_string(),
        };
        assert!(err.to_string().contains("sysbox"));
    }

    #[test]
    fn test_io_error_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: CoastError = io_err.into();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_json_error_from() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err: CoastError = json_err.into();
        assert!(err.to_string().contains("JSON serialization error"));
    }

    #[test]
    fn test_toml_error_from() {
        let toml_err = toml::from_str::<toml::Value>("{{invalid").unwrap_err();
        let err: CoastError = toml_err.into();
        assert!(err.to_string().contains("TOML parse error"));
    }

    #[test]
    fn test_dangling_container_detected_display() {
        let err = CoastError::DanglingContainerDetected {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            container_name: "my-app-coasts-dev-1".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("dangling Docker container"));
        assert!(msg.contains("my-app-coasts-dev-1"));
        assert!(msg.contains("dev-1"));
        assert!(msg.contains("my-app"));
        assert!(msg.contains("--force-remove-dangling"));
    }

    #[test]
    fn test_convenience_constructors() {
        let _ = CoastError::docker("connection refused");
        let _ = CoastError::git("not a git repository");
        let _ = CoastError::secret("decryption failed");
        let _ = CoastError::state("database locked");
        let _ = CoastError::port("port 3000 in use");
        let _ = CoastError::io("permission denied", "/etc/shadow");
        let _ = CoastError::artifact("manifest corrupted");
        let _ = CoastError::protocol("invalid request type");
    }
}
