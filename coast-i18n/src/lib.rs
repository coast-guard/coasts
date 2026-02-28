// Coast i18n — shared translation infrastructure for daemon and CLI.
//
// Uses `rust-i18n` to load JSON locale files at compile time.
// Re-exports the `t!` macro and provides language validation helpers.
//
// IMPORTANT: Each crate that uses `t!()` must also add `rust-i18n` as a
// dependency and call `rust_i18n::i18n!()` at its crate root, pointing to
// the shared locale directory: `rust_i18n::i18n!("../coast-i18n/locales", fallback = "en");`
rust_i18n::i18n!("locales", fallback = "en");

pub use rust_i18n::t;

/// All language codes supported by Coast.
pub const SUPPORTED_LANGUAGES: &[&str] = &["en", "zh", "ja", "ko", "ru", "pt", "es"];

/// Human-readable names for each supported language, indexed to match
/// [`SUPPORTED_LANGUAGES`].
pub const LANGUAGE_NAMES: &[&str] = &[
    "English",
    "中文",
    "日本語",
    "한국어",
    "Русский",
    "Português",
    "Español",
];

/// Returns `true` if the given code is a supported language.
pub fn is_valid_language(code: &str) -> bool {
    SUPPORTED_LANGUAGES.contains(&code)
}

/// Get the human-readable name for a language code.
/// Returns `None` if the code is not supported.
pub fn language_name(code: &str) -> Option<&'static str> {
    SUPPORTED_LANGUAGES
        .iter()
        .position(|&c| c == code)
        .map(|i| LANGUAGE_NAMES[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_languages_count() {
        assert_eq!(SUPPORTED_LANGUAGES.len(), 7);
        assert_eq!(LANGUAGE_NAMES.len(), 7);
    }

    #[test]
    fn test_is_valid_language_all_supported() {
        for lang in SUPPORTED_LANGUAGES {
            assert!(is_valid_language(lang), "expected {lang} to be valid");
        }
    }

    #[test]
    fn test_is_valid_language_rejects_unsupported() {
        assert!(!is_valid_language("fr"));
        assert!(!is_valid_language(""));
        assert!(!is_valid_language("english"));
        assert!(!is_valid_language("EN"));
    }

    #[test]
    fn test_language_name_english() {
        assert_eq!(language_name("en"), Some("English"));
    }

    #[test]
    fn test_language_name_chinese() {
        assert_eq!(language_name("zh"), Some("中文"));
    }

    #[test]
    fn test_language_name_unknown() {
        assert_eq!(language_name("fr"), None);
    }

    #[test]
    fn test_translation_lookup_en() {
        let result = t!(
            "error.instance_not_found",
            locale = "en",
            name = "dev-1",
            project = "my-app"
        );
        assert!(
            result.contains("dev-1"),
            "expected 'dev-1' in translated string, got: {result}"
        );
        assert!(
            result.contains("my-app"),
            "expected 'my-app' in translated string, got: {result}"
        );
    }

    #[test]
    fn test_translation_all_locales_load() {
        for &lang in SUPPORTED_LANGUAGES {
            let result = t!(
                "error.instance_not_found",
                locale = lang,
                name = "x",
                project = "p"
            );
            assert!(
                !result.is_empty(),
                "translation for locale '{lang}' returned empty string"
            );
        }
    }

    #[test]
    fn test_translation_fallback_to_english() {
        let result = t!(
            "error.instance_not_found",
            locale = "fr",
            name = "x",
            project = "p"
        );
        let en_result = t!(
            "error.instance_not_found",
            locale = "en",
            name = "x",
            project = "p"
        );
        assert_eq!(
            result, en_result,
            "unsupported locale should fall back to English"
        );
    }

    #[test]
    fn test_translation_missing_key_returns_key() {
        let result = t!("nonexistent.key.that.does.not.exist", locale = "en");
        assert!(
            result.contains("nonexistent.key"),
            "missing key should return the key itself, got: {result}"
        );
    }

    #[test]
    fn test_interpolation() {
        let result = t!(
            "error.instance_already_exists",
            locale = "en",
            name = "dev-1",
            project = "app"
        );
        assert!(result.contains("dev-1"));
        assert!(result.contains("app"));
    }

    #[test]
    fn test_cli_about_keys_exist() {
        let result = t!("cli.build.about", locale = "en");
        assert!(!result.is_empty());
        assert!(
            !result.contains("cli.build.about"),
            "key should resolve, not return itself"
        );
    }
}
