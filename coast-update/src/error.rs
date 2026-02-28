/// Errors that can occur during update checking and application.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("Failed to fetch update policy: {0}")]
    PolicyFetch(String),

    #[error("Failed to parse update policy: {0}")]
    PolicyParse(String),

    #[error("Failed to check for updates: {0}")]
    CheckFailed(String),

    #[error("Failed to parse version '{version}': {reason}")]
    VersionParse { version: String, reason: String },

    #[error("Failed to download update: {0}")]
    DownloadFailed(String),

    #[error("Failed to apply update: {0}")]
    ApplyFailed(String),

    #[error("This binary was installed via Homebrew. Run `brew upgrade coast` instead.")]
    HomebrewInstall,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let err = UpdateError::PolicyFetch("timeout".into());
        assert_eq!(err.to_string(), "Failed to fetch update policy: timeout");

        let err = UpdateError::PolicyParse("invalid json".into());
        assert_eq!(
            err.to_string(),
            "Failed to parse update policy: invalid json"
        );

        let err = UpdateError::VersionParse {
            version: "abc".into(),
            reason: "not semver".into(),
        };
        assert!(err.to_string().contains("abc"));

        let err = UpdateError::HomebrewInstall;
        assert!(err.to_string().contains("brew upgrade"));

        let err = UpdateError::DownloadFailed("404".into());
        assert!(err.to_string().contains("404"));

        let err = UpdateError::ApplyFailed("permission denied".into());
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let update_err: UpdateError = io_err.into();
        assert!(matches!(update_err, UpdateError::Io(_)));
        assert!(update_err.to_string().contains("file missing"));
    }
}
