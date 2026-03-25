/// CLI i18n helper — resolves the active language for the current session.
///
/// Resolution priority:
/// 1. `COAST_LANG` environment variable
/// 2. Persisted preference in `~/.coast/state.db` (`user_config` table)
/// 3. System locale (`LANG` / `LC_ALL` environment variables)
/// 4. Default: `"en"`
use std::sync::OnceLock;

static CACHED_LANG: OnceLock<String> = OnceLock::new();

/// Get the resolved language code for this CLI session.
///
/// The result is cached after the first call.
pub fn cli_lang() -> &'static str {
    CACHED_LANG.get_or_init(resolve_language)
}

/// Resolve language without caching (used by `cli_lang` and tests).
pub fn resolve_language() -> String {
    if let Ok(lang) = std::env::var("COAST_LANG") {
        if coast_i18n::is_valid_language(&lang) {
            return lang;
        }
    }

    if let Some(lang) = read_language_from_db() {
        return lang;
    }

    if let Some(lang) = detect_system_locale() {
        return lang;
    }

    "en".to_string()
}

/// Read the language directly from `~/.coast/state.db`.
///
/// This is a fast local file read (sub-millisecond), no daemon needed.
/// Returns `None` if the DB doesn't exist or the key isn't set.
fn read_language_from_db() -> Option<String> {
    let db_path = dirs::home_dir()?.join(".coast").join("state.db");
    if !db_path.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let lang: String = conn
        .query_row(
            "SELECT value FROM user_config WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
        .ok()?;
    if coast_i18n::is_valid_language(&lang) {
        Some(lang)
    } else {
        None
    }
}

/// Detect the language from system locale environment variables.
fn detect_system_locale() -> Option<String> {
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let code = extract_language_code(&val);
            if coast_i18n::is_valid_language(code) {
                return Some(code.to_string());
            }
        }
    }
    None
}

/// Extract the 2-letter language code from a locale string like "ja_JP.UTF-8".
fn extract_language_code(locale: &str) -> &str {
    let s = locale.split('.').next().unwrap_or(locale);
    let s = s.split('_').next().unwrap_or(s);
    s.split('-').next().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_extract_language_code_full_locale() {
        assert_eq!(extract_language_code("ja_JP.UTF-8"), "ja");
    }

    #[test]
    fn test_extract_language_code_simple() {
        assert_eq!(extract_language_code("en"), "en");
    }

    #[test]
    fn test_extract_language_code_with_region() {
        assert_eq!(extract_language_code("pt_BR"), "pt");
    }

    #[test]
    fn test_extract_language_code_hyphenated() {
        assert_eq!(extract_language_code("zh-CN"), "zh");
    }

    #[test]
    fn test_resolve_language_defaults_to_valid() {
        let _lock = ENV_LOCK.lock().unwrap();
        let saved_lang = std::env::var("COAST_LANG").ok();
        std::env::remove_var("COAST_LANG");
        let saved_lc = std::env::var("LC_ALL").ok();
        std::env::remove_var("LC_ALL");
        let saved_lang_env = std::env::var("LANG").ok();
        std::env::remove_var("LANG");
        let saved_lc_msg = std::env::var("LC_MESSAGES").ok();
        std::env::remove_var("LC_MESSAGES");

        let lang = resolve_language();
        // Should return a valid language — either "en" (no DB) or whatever is
        // stored in the local state.db if one exists on this machine.
        assert!(
            coast_i18n::is_valid_language(&lang),
            "expected a valid language code, got: {lang}"
        );

        // Restore env
        if let Some(v) = saved_lang {
            std::env::set_var("COAST_LANG", v);
        }
        if let Some(v) = saved_lc {
            std::env::set_var("LC_ALL", v);
        }
        if let Some(v) = saved_lang_env {
            std::env::set_var("LANG", v);
        }
        if let Some(v) = saved_lc_msg {
            std::env::set_var("LC_MESSAGES", v);
        }
    }

    #[test]
    fn test_resolve_language_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let saved = std::env::var("COAST_LANG").ok();
        std::env::set_var("COAST_LANG", "ja");

        let lang = resolve_language();
        assert_eq!(lang, "ja");

        // Restore
        match saved {
            Some(v) => std::env::set_var("COAST_LANG", v),
            None => std::env::remove_var("COAST_LANG"),
        }
    }

    #[test]
    fn test_resolve_language_invalid_env_falls_through() {
        let _lock = ENV_LOCK.lock().unwrap();
        let saved = std::env::var("COAST_LANG").ok();
        std::env::set_var("COAST_LANG", "fr");

        let saved_lc = std::env::var("LC_ALL").ok();
        std::env::remove_var("LC_ALL");
        let saved_lang_env = std::env::var("LANG").ok();
        std::env::remove_var("LANG");
        let saved_lc_msg = std::env::var("LC_MESSAGES").ok();
        std::env::remove_var("LC_MESSAGES");

        let lang = resolve_language();
        // "fr" is invalid, so it falls through. Result depends on whether
        // ~/.coast/state.db exists with a stored preference (returns that)
        // or not (returns "en").
        assert!(
            coast_i18n::is_valid_language(&lang),
            "expected a valid language code, got: {lang}"
        );
        assert_ne!(lang, "fr", "invalid locale should never be returned");

        // Restore
        match saved {
            Some(v) => std::env::set_var("COAST_LANG", v),
            None => std::env::remove_var("COAST_LANG"),
        }
        if let Some(v) = saved_lc {
            std::env::set_var("LC_ALL", v);
        }
        if let Some(v) = saved_lang_env {
            std::env::set_var("LANG", v);
        }
        if let Some(v) = saved_lc_msg {
            std::env::set_var("LC_MESSAGES", v);
        }
    }

    #[test]
    fn test_resolve_language_env_takes_priority() {
        let saved = std::env::var("COAST_LANG").ok();
        std::env::set_var("COAST_LANG", "es");

        let lang = resolve_language();
        assert_eq!(lang, "es");

        match saved {
            Some(v) => std::env::set_var("COAST_LANG", v),
            None => std::env::remove_var("COAST_LANG"),
        }
    }

    #[test]
    fn test_read_language_from_db_with_temp_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE user_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO user_config (key, value) VALUES ('language', 'ko');",
        )
        .unwrap();
        drop(conn);

        let read_conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let lang: String = read_conn
            .query_row(
                "SELECT value FROM user_config WHERE key = 'language'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lang, "ko");
    }

    #[test]
    fn test_read_language_from_db_missing_table() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        drop(conn);

        let read_conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let result: Result<String, _> = read_conn.query_row(
            "SELECT value FROM user_config WHERE key = 'language'",
            [],
            |row| row.get(0),
        );
        assert!(result.is_err(), "should fail when table doesn't exist");
    }
}
