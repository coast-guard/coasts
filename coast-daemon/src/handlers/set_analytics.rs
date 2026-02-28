/// Handler for the `SetAnalytics` request.
///
/// Reads or updates the analytics setting based on the requested action,
/// persists it to `user_config`, and emits a `ConfigAnalyticsChanged` event
/// when the value changes.
use coast_core::error::Result;
use coast_core::protocol::{
    AnalyticsAction, CoastEvent, SetAnalyticsRequest, SetAnalyticsResponse,
};

use crate::server::AppState;

pub async fn handle(req: SetAnalyticsRequest, state: &AppState) -> Result<SetAnalyticsResponse> {
    let db = state.db.lock().await;

    match req.action {
        AnalyticsAction::Enable => {
            db.set_analytics_enabled(true)?;
            drop(db);
            let _ = state.analytics_enabled_tx.send(true);
            state.emit_event(CoastEvent::ConfigAnalyticsChanged { enabled: true });
            Ok(SetAnalyticsResponse { enabled: true })
        }
        AnalyticsAction::Disable => {
            db.set_analytics_enabled(false)?;
            drop(db);
            let _ = state.analytics_enabled_tx.send(false);
            state.emit_event(CoastEvent::ConfigAnalyticsChanged { enabled: false });
            Ok(SetAnalyticsResponse { enabled: false })
        }
        AnalyticsAction::Status => {
            let enabled = db.get_analytics_enabled()?;
            Ok(SetAnalyticsResponse { enabled })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn test_state() -> AppState {
        let db = StateDb::open_in_memory().unwrap();
        AppState::new_for_testing(db)
    }

    #[tokio::test]
    async fn test_default_is_enabled() {
        let state = test_state();
        let resp = handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Status,
            },
            &state,
        )
        .await
        .unwrap();
        assert!(resp.enabled);
    }

    #[tokio::test]
    async fn test_enable() {
        let state = test_state();
        let resp = handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Enable,
            },
            &state,
        )
        .await
        .unwrap();
        assert!(resp.enabled);

        // Verify persistence
        let status = handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Status,
            },
            &state,
        )
        .await
        .unwrap();
        assert!(status.enabled);
    }

    #[tokio::test]
    async fn test_disable() {
        let state = test_state();
        // Enable first
        handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Enable,
            },
            &state,
        )
        .await
        .unwrap();

        let resp = handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Disable,
            },
            &state,
        )
        .await
        .unwrap();
        assert!(!resp.enabled);
    }

    #[tokio::test]
    async fn test_enable_emits_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Enable,
            },
            &state,
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        match event {
            CoastEvent::ConfigAnalyticsChanged { enabled } => assert!(enabled),
            _ => panic!("expected ConfigAnalyticsChanged event"),
        }
    }

    #[tokio::test]
    async fn test_disable_emits_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Disable,
            },
            &state,
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        match event {
            CoastEvent::ConfigAnalyticsChanged { enabled } => assert!(!enabled),
            _ => panic!("expected ConfigAnalyticsChanged event"),
        }
    }

    #[tokio::test]
    async fn test_status_does_not_emit_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        handle(
            SetAnalyticsRequest {
                action: AnalyticsAction::Status,
            },
            &state,
        )
        .await
        .unwrap();

        assert!(rx.try_recv().is_err());
    }
}
