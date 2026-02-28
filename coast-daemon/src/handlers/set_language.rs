/// Handler for the `SetLanguage` request.
///
/// Validates the language code, persists it to `user_config`, updates the
/// in-memory language cache, and emits a `ConfigLanguageChanged` event.
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, SetLanguageRequest, SetLanguageResponse};

use crate::server::AppState;

pub async fn handle(req: SetLanguageRequest, state: &AppState) -> Result<SetLanguageResponse> {
    if !coast_i18n::is_valid_language(&req.language) {
        return Err(CoastError::state(format!(
            "Unsupported language '{}'. Supported languages: {}",
            req.language,
            coast_i18n::SUPPORTED_LANGUAGES.join(", "),
        )));
    }

    let db = state.db.lock().await;
    db.set_language(&req.language)?;
    drop(db);

    let _ = state.language_tx.send(req.language.clone());

    state.emit_event(CoastEvent::ConfigLanguageChanged {
        language: req.language.clone(),
    });

    Ok(SetLanguageResponse {
        language: req.language,
    })
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
    async fn test_set_language_valid() {
        let state = test_state();
        let resp = handle(
            SetLanguageRequest {
                language: "zh".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.language, "zh");
        assert_eq!(state.language(), "zh");
    }

    #[tokio::test]
    async fn test_set_language_invalid() {
        let state = test_state();
        let result = handle(
            SetLanguageRequest {
                language: "fr".to_string(),
            },
            &state,
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported language"));
    }

    #[tokio::test]
    async fn test_set_language_emits_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        handle(
            SetLanguageRequest {
                language: "ja".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        match event {
            CoastEvent::ConfigLanguageChanged { language } => {
                assert_eq!(language, "ja");
            }
            _ => panic!("expected ConfigLanguageChanged event"),
        }
    }

    #[tokio::test]
    async fn test_set_language_persists() {
        let state = test_state();
        handle(
            SetLanguageRequest {
                language: "ko".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        let db = state.db.lock().await;
        assert_eq!(db.get_language().unwrap(), "ko");
    }
}
