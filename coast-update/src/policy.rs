/// Update policy enforcement — three-tier policy (nudge/required/auto).
///
/// The policy is fetched from a JSON file hosted on GitHub (raw content).
/// It determines how the CLI behaves when a newer version is available.
use crate::error::UpdateError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const POLICY_URL: &str =
    "https://raw.githubusercontent.com/coast-guard/coasts/main/cli-update-policy.json";

/// The three update policy tiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyTier {
    /// Print a message after command execution suggesting an update.
    Nudge,
    /// Block CLI execution until the user updates (unless on minimum_version or above).
    Required,
    /// Automatically download and apply the update before running the command.
    Auto,
}

impl fmt::Display for PolicyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyTier::Nudge => write!(f, "nudge"),
            PolicyTier::Required => write!(f, "required"),
            PolicyTier::Auto => write!(f, "auto"),
        }
    }
}

/// The update policy document fetched from the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicy {
    pub policy: PolicyTier,
    pub minimum_version: String,
    /// Optional custom message to display to the user.
    #[serde(default)]
    pub message: String,
}

impl Default for UpdatePolicy {
    fn default() -> Self {
        Self {
            policy: PolicyTier::Nudge,
            minimum_version: "0.0.0".to_string(),
            message: String::new(),
        }
    }
}

/// The result of evaluating the update policy against the current version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction {
    /// No update needed — current version is up to date.
    UpToDate,
    /// A newer version exists; show a post-command message.
    Nudge {
        current: String,
        latest: String,
        message: String,
    },
    /// The current version is below minimum; block execution.
    Required {
        current: String,
        minimum: String,
        message: String,
    },
    /// Automatically update before proceeding.
    AutoUpdate {
        current: String,
        latest: String,
        message: String,
    },
}

/// Fetch the update policy from the remote URL.
///
/// Returns the default (nudge) policy on any network or parse failure,
/// so that the CLI never blocks due to a policy fetch error.
pub async fn fetch_policy(timeout: Duration) -> UpdatePolicy {
    match fetch_policy_inner(timeout).await {
        Ok(policy) => policy,
        Err(e) => {
            tracing::debug!("Failed to fetch update policy, using default: {e}");
            UpdatePolicy::default()
        }
    }
}

async fn fetch_policy_inner(timeout: Duration) -> Result<UpdatePolicy, UpdateError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent("coast-cli")
        .build()
        .map_err(|e| UpdateError::PolicyFetch(e.to_string()))?;

    let resp = client
        .get(POLICY_URL)
        .send()
        .await
        .map_err(|e| UpdateError::PolicyFetch(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(UpdateError::PolicyFetch(format!("HTTP {}", resp.status())));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| UpdateError::PolicyFetch(e.to_string()))?;

    serde_json::from_str(&text).map_err(|e| UpdateError::PolicyParse(e.to_string()))
}

/// Evaluate the update policy against the current and latest versions.
///
/// `current` and `latest` are semver version strings (with or without 'v' prefix).
pub fn evaluate_policy(
    policy: &UpdatePolicy,
    current: &semver::Version,
    latest: Option<&semver::Version>,
) -> PolicyAction {
    let minimum = crate::version::parse_version(&policy.minimum_version).ok();

    // If current version is below the minimum, always require update
    if let Some(ref min) = minimum {
        if current < min {
            return PolicyAction::Required {
                current: current.to_string(),
                minimum: min.to_string(),
                message: policy.message.clone(),
            };
        }
    }

    // If we don't know the latest version, or we're up to date, nothing to do
    let Some(latest) = latest else {
        return PolicyAction::UpToDate;
    };

    if !crate::version::is_newer(current, latest) {
        return PolicyAction::UpToDate;
    }

    match policy.policy {
        PolicyTier::Nudge => PolicyAction::Nudge {
            current: current.to_string(),
            latest: latest.to_string(),
            message: policy.message.clone(),
        },
        PolicyTier::Required => PolicyAction::Required {
            current: current.to_string(),
            minimum: latest.to_string(),
            message: policy.message.clone(),
        },
        PolicyTier::Auto => PolicyAction::AutoUpdate {
            current: current.to_string(),
            latest: latest.to_string(),
            message: policy.message.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn v(s: &str) -> Version {
        crate::version::parse_version(s).unwrap()
    }

    #[test]
    fn test_default_policy() {
        let policy = UpdatePolicy::default();
        assert_eq!(policy.policy, PolicyTier::Nudge);
        assert_eq!(policy.minimum_version, "0.0.0");
        assert!(policy.message.is_empty());
    }

    #[test]
    fn test_policy_tier_display() {
        assert_eq!(PolicyTier::Nudge.to_string(), "nudge");
        assert_eq!(PolicyTier::Required.to_string(), "required");
        assert_eq!(PolicyTier::Auto.to_string(), "auto");
    }

    #[test]
    fn test_policy_serialization_roundtrip() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Required,
            minimum_version: "0.2.0".to_string(),
            message: "Please update".to_string(),
        };
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: UpdatePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.policy, PolicyTier::Required);
        assert_eq!(parsed.minimum_version, "0.2.0");
        assert_eq!(parsed.message, "Please update");
    }

    #[test]
    fn test_policy_deserialization_from_json() {
        let json = r#"{"policy": "nudge", "minimum_version": "0.1.0", "message": ""}"#;
        let policy: UpdatePolicy = serde_json::from_str(json).unwrap();
        assert_eq!(policy.policy, PolicyTier::Nudge);
        assert_eq!(policy.minimum_version, "0.1.0");
    }

    #[test]
    fn test_policy_deserialization_missing_message() {
        let json = r#"{"policy": "auto", "minimum_version": "0.1.0"}"#;
        let policy: UpdatePolicy = serde_json::from_str(json).unwrap();
        assert_eq!(policy.policy, PolicyTier::Auto);
        assert!(policy.message.is_empty());
    }

    #[test]
    fn test_evaluate_up_to_date() {
        let policy = UpdatePolicy::default();
        let current = v("0.2.0");
        let latest = v("0.2.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert_eq!(action, PolicyAction::UpToDate);
    }

    #[test]
    fn test_evaluate_up_to_date_no_latest() {
        let policy = UpdatePolicy::default();
        let current = v("0.1.0");
        let action = evaluate_policy(&policy, &current, None);
        assert_eq!(action, PolicyAction::UpToDate);
    }

    #[test]
    fn test_evaluate_nudge() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Nudge,
            minimum_version: "0.0.0".to_string(),
            message: "Update available!".to_string(),
        };
        let current = v("0.1.0");
        let latest = v("0.2.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert!(matches!(action, PolicyAction::Nudge { .. }));
        if let PolicyAction::Nudge {
            current: c,
            latest: l,
            message: m,
        } = action
        {
            assert_eq!(c, "0.1.0");
            assert_eq!(l, "0.2.0");
            assert_eq!(m, "Update available!");
        }
    }

    #[test]
    fn test_evaluate_required_by_tier() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Required,
            minimum_version: "0.0.0".to_string(),
            message: String::new(),
        };
        let current = v("0.1.0");
        let latest = v("0.2.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert!(matches!(action, PolicyAction::Required { .. }));
    }

    #[test]
    fn test_evaluate_required_by_minimum_version() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Nudge, // even nudge triggers Required if below minimum
            minimum_version: "0.3.0".to_string(),
            message: "Critical fix".to_string(),
        };
        let current = v("0.1.0");
        let latest = v("0.3.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert!(matches!(action, PolicyAction::Required { .. }));
        if let PolicyAction::Required { minimum, .. } = action {
            assert_eq!(minimum, "0.3.0");
        }
    }

    #[test]
    fn test_evaluate_required_by_minimum_no_latest() {
        // Even without knowing the latest, if below minimum, block
        let policy = UpdatePolicy {
            policy: PolicyTier::Nudge,
            minimum_version: "0.5.0".to_string(),
            message: String::new(),
        };
        let current = v("0.1.0");
        let action = evaluate_policy(&policy, &current, None);
        assert!(matches!(action, PolicyAction::Required { .. }));
    }

    #[test]
    fn test_evaluate_auto() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Auto,
            minimum_version: "0.0.0".to_string(),
            message: String::new(),
        };
        let current = v("0.1.0");
        let latest = v("0.2.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert!(matches!(action, PolicyAction::AutoUpdate { .. }));
    }

    #[test]
    fn test_evaluate_above_minimum_up_to_date() {
        let policy = UpdatePolicy {
            policy: PolicyTier::Required,
            minimum_version: "0.1.0".to_string(),
            message: String::new(),
        };
        let current = v("0.2.0");
        let latest = v("0.2.0");
        let action = evaluate_policy(&policy, &current, Some(&latest));
        assert_eq!(action, PolicyAction::UpToDate);
    }
}
