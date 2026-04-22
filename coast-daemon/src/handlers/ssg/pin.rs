//! `coast ssg checkout-build` / `uncheckout-build` / `show-pin`
//! handlers.
//!
//! Phase: ssg-phase-16. See `DESIGN.md §17-9` (SETTLED — Phase 16).
//!
//! Thin adapter: validates the pinnable build (for checkout-build),
//! then reads/writes the `ssg_consumer_pins` state row. All
//! pin-resolution business lives in [`coast_ssg::runtime::pinning`].

use std::sync::Arc;

use coast_core::error::Result;
use coast_core::protocol::SsgResponse;
use coast_ssg::state::{SsgConsumerPinRecord, SsgStateExt};

use crate::server::AppState;

const STATUS_PINNED: &str = "pinned";
const STATUS_UNPINNED: &str = "unpinned";
const STATUS_NO_PIN: &str = "no-pin";

pub(super) async fn handle_checkout_build(
    state: &Arc<AppState>,
    project: String,
    build_id: String,
) -> Result<SsgResponse> {
    // Validate the build exists on disk before we write the pin row.
    // Fail fast on typos or pruned builds.
    let manifest = coast_ssg::runtime::pinning::validate_pinnable_build(&build_id)?;

    let now = chrono::Utc::now().to_rfc3339();
    let rec = SsgConsumerPinRecord {
        project: project.clone(),
        build_id: build_id.clone(),
        created_at: now,
    };
    {
        let db = state.db.lock().await;
        db.upsert_ssg_consumer_pin(&rec)?;
    }

    let service_count = manifest.services.len();
    Ok(SsgResponse {
        message: format!(
            "Pinned project '{project}' to SSG build {build_id} ({service_count} service(s)).\n\
             Drift checks and auto-start will use this build until you run \
             `coast ssg uncheckout-build`."
        ),
        status: Some(STATUS_PINNED.to_string()),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    })
}

pub(super) async fn handle_uncheckout_build(
    state: &Arc<AppState>,
    project: String,
) -> Result<SsgResponse> {
    let had_pin = {
        let db = state.db.lock().await;
        db.delete_ssg_consumer_pin(&project)?
    };

    let (message, status) = if had_pin {
        (
            format!(
                "Unpinned project '{project}'. Drift checks and auto-start will now use the \
                 latest SSG build."
            ),
            STATUS_UNPINNED,
        )
    } else {
        (
            format!("No SSG build pin found for project '{project}'; nothing to do."),
            STATUS_NO_PIN,
        )
    };
    Ok(SsgResponse {
        message,
        status: Some(status.to_string()),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    })
}

pub(super) async fn handle_show_pin(state: &Arc<AppState>, project: String) -> Result<SsgResponse> {
    let pin = {
        let db = state.db.lock().await;
        db.get_ssg_consumer_pin(&project)?
    };

    let (message, status) = match pin {
        Some(p) => (
            format!(
                "Project '{project}' is pinned to SSG build {build_id} (pinned at \
                 {created_at}).",
                build_id = p.build_id,
                created_at = p.created_at,
            ),
            STATUS_PINNED,
        ),
        None => (
            format!(
                "No SSG build pin for project '{project}'. Drift and auto-start use the \
                 latest build."
            ),
            STATUS_NO_PIN,
        ),
    };
    Ok(SsgResponse {
        message,
        status: Some(status.to_string()),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn in_memory_app_state() -> Arc<AppState> {
        let db = StateDb::open_in_memory().expect("in-memory statedb");
        Arc::new(AppState::new_for_testing(db))
    }

    #[tokio::test]
    async fn uncheckout_no_pin_reports_no_pin_status() {
        let state = in_memory_app_state();
        let resp = handle_uncheckout_build(&state, "proj".to_string())
            .await
            .unwrap();
        assert_eq!(resp.status.as_deref(), Some(STATUS_NO_PIN));
        assert!(resp.message.contains("nothing to do"));
    }

    #[tokio::test]
    async fn show_pin_no_pin_reports_no_pin_status() {
        let state = in_memory_app_state();
        let resp = handle_show_pin(&state, "proj".to_string()).await.unwrap();
        assert_eq!(resp.status.as_deref(), Some(STATUS_NO_PIN));
        assert!(resp.message.contains("No SSG build pin"));
    }

    #[tokio::test]
    async fn uncheckout_after_pin_reports_unpinned_status() {
        let state = in_memory_app_state();
        // Seed a pin directly in the DB (bypasses checkout_build's
        // build-dir existence check, which is out of scope here).
        {
            let db = state.db.lock().await;
            db.upsert_ssg_consumer_pin(&SsgConsumerPinRecord {
                project: "proj".to_string(),
                build_id: "b1".to_string(),
                created_at: "2026-04-22T00:00:00Z".to_string(),
            })
            .unwrap();
        }
        let resp = handle_uncheckout_build(&state, "proj".to_string())
            .await
            .unwrap();
        assert_eq!(resp.status.as_deref(), Some(STATUS_UNPINNED));
        assert!(resp.message.contains("Unpinned project 'proj'"));
    }

    #[tokio::test]
    async fn show_pin_after_pin_reports_pinned_with_build_id() {
        let state = in_memory_app_state();
        {
            let db = state.db.lock().await;
            db.upsert_ssg_consumer_pin(&SsgConsumerPinRecord {
                project: "proj".to_string(),
                build_id: "b1_20260422".to_string(),
                created_at: "2026-04-22T00:00:00Z".to_string(),
            })
            .unwrap();
        }
        let resp = handle_show_pin(&state, "proj".to_string()).await.unwrap();
        assert_eq!(resp.status.as_deref(), Some(STATUS_PINNED));
        assert!(resp.message.contains("b1_20260422"));
        assert!(resp.message.contains("proj"));
    }

    #[tokio::test]
    async fn uncheckout_only_affects_named_project() {
        let state = in_memory_app_state();
        {
            let db = state.db.lock().await;
            db.upsert_ssg_consumer_pin(&SsgConsumerPinRecord {
                project: "proj-a".to_string(),
                build_id: "ba".to_string(),
                created_at: "ts".to_string(),
            })
            .unwrap();
            db.upsert_ssg_consumer_pin(&SsgConsumerPinRecord {
                project: "proj-b".to_string(),
                build_id: "bb".to_string(),
                created_at: "ts".to_string(),
            })
            .unwrap();
        }
        handle_uncheckout_build(&state, "proj-a".to_string())
            .await
            .unwrap();

        let db = state.db.lock().await;
        assert!(db.get_ssg_consumer_pin("proj-a").unwrap().is_none());
        assert!(db.get_ssg_consumer_pin("proj-b").unwrap().is_some());
    }
}
