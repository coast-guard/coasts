/// The current version of the coast CLI, set at compile time from Cargo.toml.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::error::UpdateError;
use semver::Version;

/// Parse a version string into a semver `Version`.
///
/// Strips a leading 'v' if present (e.g. "v0.1.0" -> "0.1.0").
pub fn parse_version(s: &str) -> Result<Version, UpdateError> {
    let cleaned = s.strip_prefix('v').unwrap_or(s);
    Version::parse(cleaned).map_err(|e| UpdateError::VersionParse {
        version: s.to_string(),
        reason: e.to_string(),
    })
}

/// Return the current binary version as a parsed `Version`.
pub fn current_version() -> Result<Version, UpdateError> {
    parse_version(CURRENT_VERSION)
}

/// Returns true if `latest` is newer than `current`.
pub fn is_newer(current: &Version, latest: &Version) -> bool {
    latest > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_version_parses() {
        let v = current_version().unwrap();
        // Just verify it parses as valid semver
        assert!(v.major < 100, "version should be reasonable");
    }

    #[test]
    fn test_current_version_const() {
        // Verify CURRENT_VERSION is valid semver, don't pin to a specific value
        assert!(parse_version(CURRENT_VERSION).is_ok());
    }

    #[test]
    fn test_parse_version_plain() {
        let v = parse_version("1.2.3").unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn test_parse_version_with_v_prefix() {
        let v = parse_version("v1.2.3").unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn test_parse_version_prerelease() {
        let v = parse_version("v0.1.0-rc.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
        assert!(!v.pre.is_empty());
    }

    #[test]
    fn test_parse_version_invalid() {
        let err = parse_version("not-a-version").unwrap_err();
        assert!(matches!(err, UpdateError::VersionParse { .. }));
    }

    #[test]
    fn test_parse_version_empty() {
        let err = parse_version("").unwrap_err();
        assert!(matches!(err, UpdateError::VersionParse { .. }));
    }

    #[test]
    fn test_is_newer_true() {
        let current = Version::new(0, 1, 0);
        let latest = Version::new(0, 2, 0);
        assert!(is_newer(&current, &latest));
    }

    #[test]
    fn test_is_newer_false_same() {
        let current = Version::new(0, 1, 0);
        let latest = Version::new(0, 1, 0);
        assert!(!is_newer(&current, &latest));
    }

    #[test]
    fn test_is_newer_false_older() {
        let current = Version::new(1, 0, 0);
        let latest = Version::new(0, 9, 0);
        assert!(!is_newer(&current, &latest));
    }

    #[test]
    fn test_is_newer_prerelease_less_than_release() {
        // semver: 0.1.0-rc.1 < 0.1.0
        let current = Version::new(0, 1, 0);
        let latest = parse_version("0.1.0-rc.1").unwrap();
        assert!(!is_newer(&current, &latest));
    }
}
