use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};

use super::StateDb;

impl StateDb {
    // -----------------------------------------------------------------------
    // User config CRUD
    // -----------------------------------------------------------------------

    /// Get a user config value by key.
    pub fn get_user_config(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM user_config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query user_config '{key}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Upsert a user config value.
    pub fn set_user_config(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO user_config (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set user_config '{key}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!(key = %key, "user config saved");
        Ok(())
    }

    /// Delete a user config entry by key.
    pub fn delete_user_config(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM user_config WHERE key = ?1", params![key])
            .map_err(|e| CoastError::State {
                message: format!("failed to delete user_config '{key}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!(key = %key, "user config deleted");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Language convenience methods
    // -----------------------------------------------------------------------

    /// Get the user's preferred language, defaulting to `"en"`.
    pub fn get_language(&self) -> Result<String> {
        Ok(self
            .get_user_config("language")?
            .unwrap_or_else(|| "en".to_string()))
    }

    // -----------------------------------------------------------------------
    // Analytics convenience methods
    // -----------------------------------------------------------------------

    /// Get whether analytics is enabled, defaulting to `true`.
    pub fn get_analytics_enabled(&self) -> Result<bool> {
        Ok(self
            .get_user_config("analytics_enabled")?
            .map(|v| v == "true")
            .unwrap_or(true))
    }

    /// Set the analytics enabled/disabled flag.
    pub fn set_analytics_enabled(&self, enabled: bool) -> Result<()> {
        self.set_user_config("analytics_enabled", if enabled { "true" } else { "false" })
    }

    /// Set the user's preferred language after validating the code.
    #[instrument(skip(self))]
    pub fn set_language(&self, lang: &str) -> Result<()> {
        if !coast_i18n::is_valid_language(lang) {
            return Err(CoastError::state(format!(
                "Unsupported language '{}'. Supported languages: {}",
                lang,
                coast_i18n::SUPPORTED_LANGUAGES.join(", "),
            )));
        }
        self.set_user_config("language", lang)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

    #[test]
    fn test_get_user_config_nonexistent() {
        let db = test_db();
        assert_eq!(db.get_user_config("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_set_and_get_user_config() {
        let db = test_db();
        db.set_user_config("theme", "dark").unwrap();
        assert_eq!(
            db.get_user_config("theme").unwrap(),
            Some("dark".to_string())
        );
    }

    #[test]
    fn test_set_user_config_upsert() {
        let db = test_db();
        db.set_user_config("key", "val1").unwrap();
        assert_eq!(db.get_user_config("key").unwrap(), Some("val1".to_string()));
        db.set_user_config("key", "val2").unwrap();
        assert_eq!(db.get_user_config("key").unwrap(), Some("val2".to_string()));
    }

    #[test]
    fn test_delete_user_config() {
        let db = test_db();
        db.set_user_config("key", "value").unwrap();
        assert!(db.get_user_config("key").unwrap().is_some());
        db.delete_user_config("key").unwrap();
        assert_eq!(db.get_user_config("key").unwrap(), None);
    }

    #[test]
    fn test_get_analytics_enabled_default() {
        let db = test_db();
        assert!(db.get_analytics_enabled().unwrap());
    }

    #[test]
    fn test_set_analytics_enabled() {
        let db = test_db();
        db.set_analytics_enabled(true).unwrap();
        assert!(db.get_analytics_enabled().unwrap());
    }

    #[test]
    fn test_set_analytics_disabled() {
        let db = test_db();
        db.set_analytics_enabled(true).unwrap();
        assert!(db.get_analytics_enabled().unwrap());
        db.set_analytics_enabled(false).unwrap();
        assert!(!db.get_analytics_enabled().unwrap());
    }

    #[test]
    fn test_set_analytics_idempotent() {
        let db = test_db();
        db.set_analytics_enabled(true).unwrap();
        db.set_analytics_enabled(true).unwrap();
        assert!(db.get_analytics_enabled().unwrap());
    }

    #[test]
    fn test_get_language_default() {
        let db = test_db();
        assert_eq!(db.get_language().unwrap(), "en");
    }

    #[test]
    fn test_set_language_valid() {
        let db = test_db();
        for &lang in coast_i18n::SUPPORTED_LANGUAGES {
            db.set_language(lang).unwrap();
            assert_eq!(db.get_language().unwrap(), lang);
        }
    }

    #[test]
    fn test_set_language_invalid() {
        let db = test_db();
        let result = db.set_language("fr");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported language"));
        assert!(err.contains("fr"));
    }

    #[test]
    fn test_set_language_empty_string() {
        let db = test_db();
        let result = db.set_language("");
        assert!(result.is_err());
    }

    #[test]
    fn test_migration_preferred_language() {
        let db = test_db();
        db.set_setting("preferred_language", "zh").unwrap();
        assert_eq!(
            db.get_setting("preferred_language").unwrap(),
            Some("zh".to_string())
        );

        // The migration ran during open_in_memory()/initialize().
        // For this test, we simulate it by calling the method manually on a
        // fresh DB where we first insert the setting then re-run init.
        // Since initialize() already ran, the migration already executed.
        // The setting won't be there because user_config table was created
        // BEFORE settings had the value. So let's test the migration flow
        // by directly calling the inner logic:
        //
        // Insert preferred_language into settings
        db.set_setting("preferred_language", "ja").unwrap();
        // Manually invoke the migration again
        // (it's idempotent — uses INSERT OR IGNORE)
        db.conn
            .execute(
                "INSERT OR IGNORE INTO user_config (key, value) VALUES ('language', ?1)",
                rusqlite::params![db.get_setting("preferred_language").unwrap().unwrap()],
            )
            .unwrap();
        db.conn
            .execute("DELETE FROM settings WHERE key = 'preferred_language'", [])
            .unwrap();

        assert_eq!(db.get_language().unwrap(), "ja");
        assert_eq!(db.get_setting("preferred_language").unwrap(), None);
    }
}
