//! Environment variable interpolation for Coastfile content.
//!
//! Processes raw TOML text before parsing, substituting `${VAR}` and
//! `${VAR:-default}` references with their environment variable values.
//!
//! Syntax:
//! - `${VAR}` -- replaced with the value of env var `VAR`
//! - `${VAR:-fallback}` -- replaced with `VAR` if set, otherwise `fallback`
//! - `$${...}` -- escape: produces the literal text `${...}`
//!
//! Undefined variables without a default are replaced with an empty string
//! and a warning is collected.

/// Result of interpolating environment variables in a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpolationResult {
    /// The string with all `${VAR}` references resolved.
    pub content: String,
    /// Warnings for undefined variables that had no default.
    pub warnings: Vec<String>,
}

/// Interpolate `${VAR}` and `${VAR:-default}` references in `input`.
///
/// Variable names must start with an ASCII letter or underscore, followed
/// by any combination of ASCII letters, digits, and underscores.
pub fn interpolate_env_vars(input: &str) -> InterpolationResult {
    interpolate_with_resolver(input, |name| std::env::var(name))
}

/// Testable core: same logic as [`interpolate_env_vars`] but accepts an
/// arbitrary variable resolver instead of reading `std::env`.
fn interpolate_with_resolver<F>(input: &str, resolver: F) -> InterpolationResult
where
    F: Fn(&str) -> std::result::Result<String, std::env::VarError>,
{
    let mut result = String::with_capacity(input.len());
    let mut warnings: Vec<String> = Vec::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
            if i > 0 && bytes[i - 1] == b'$' {
                // Already pushed the first '$' in the previous iteration;
                // this is the `$${` escape. The leading '$' is already in
                // `result`, so just push '{' and continue scanning for the
                // closing '}' to emit everything literally.
                i += 1; // skip second '$', now at '{'
                result.push('{');
                i += 1;
                let literal_start = i;
                while i < len && bytes[i] != b'}' {
                    i += 1;
                }
                result.push_str(&input[literal_start..i]);
                if i < len {
                    result.push('}');
                    i += 1; // skip '}'
                }
                continue;
            }

            // Start of a `${...}` reference.
            let ref_start = i;
            i += 2; // skip "${"

            let name_start = i;
            if i < len && is_var_start(bytes[i]) {
                i += 1;
                while i < len && is_var_cont(bytes[i]) {
                    i += 1;
                }
            }
            let name = &input[name_start..i];

            if name.is_empty() {
                // Not a valid variable reference — emit literally.
                result.push_str(&input[ref_start..i]);
                continue;
            }

            // Check for `:-default` suffix.
            let default_value = if i + 1 < len && bytes[i] == b':' && bytes[i + 1] == b'-' {
                i += 2; // skip ":-"
                let default_start = i;
                while i < len && bytes[i] != b'}' {
                    i += 1;
                }
                Some(&input[default_start..i])
            } else {
                None
            };

            if i < len && bytes[i] == b'}' {
                i += 1; // skip '}'
                match resolver(name) {
                    Ok(value) => result.push_str(&value),
                    Err(_) => {
                        if let Some(default) = default_value {
                            result.push_str(default);
                        } else {
                            warnings.push(format!(
                                "undefined environment variable '{}' replaced with empty string",
                                name
                            ));
                        }
                    }
                }
            } else {
                // Unterminated `${...` — emit literally.
                result.push_str(&input[ref_start..i]);
            }
        } else {
            result.push(input[i..].chars().next().unwrap());
            i += input[i..].chars().next().unwrap().len_utf8();
        }
    }

    InterpolationResult {
        content: result,
        warnings,
    }
}

fn is_var_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_var_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn resolver_from<'a>(
        vars: &'a HashMap<&'a str, &'a str>,
    ) -> impl Fn(&str) -> Result<String, std::env::VarError> + 'a {
        move |name: &str| {
            vars.get(name)
                .map(|v| v.to_string())
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn test_no_variables() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("hello world", resolver_from(&vars));
        assert_eq!(r.content, "hello world");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_simple_substitution() {
        let mut vars = HashMap::new();
        vars.insert("NAME", "coast");
        let r = interpolate_with_resolver("hello ${NAME}", resolver_from(&vars));
        assert_eq!(r.content, "hello coast");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_multiple_variables() {
        let mut vars = HashMap::new();
        vars.insert("FIRST", "hello");
        vars.insert("SECOND", "world");
        let r = interpolate_with_resolver("${FIRST} ${SECOND}!", resolver_from(&vars));
        assert_eq!(r.content, "hello world!");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_adjacent_variables() {
        let mut vars = HashMap::new();
        vars.insert("A", "foo");
        vars.insert("B", "bar");
        let r = interpolate_with_resolver("${A}${B}", resolver_from(&vars));
        assert_eq!(r.content, "foobar");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_default_value_used_when_unset() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("port = ${PORT:-3000}", resolver_from(&vars));
        assert_eq!(r.content, "port = 3000");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_default_value_ignored_when_set() {
        let mut vars = HashMap::new();
        vars.insert("PORT", "8080");
        let r = interpolate_with_resolver("port = ${PORT:-3000}", resolver_from(&vars));
        assert_eq!(r.content, "port = 8080");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_empty_default_value() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("val = \"${X:-}\"", resolver_from(&vars));
        assert_eq!(r.content, "val = \"\"");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_undefined_without_default_warns() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("key = ${MISSING}", resolver_from(&vars));
        assert_eq!(r.content, "key = ");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("MISSING"));
    }

    #[test]
    fn test_multiple_undefined_collect_warnings() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("${A} and ${B}", resolver_from(&vars));
        assert_eq!(r.content, " and ");
        assert_eq!(r.warnings.len(), 2);
        assert!(r.warnings[0].contains("'A'"));
        assert!(r.warnings[1].contains("'B'"));
    }

    #[test]
    fn test_escape_double_dollar() {
        let mut vars = HashMap::new();
        vars.insert("VAR", "value");
        let r = interpolate_with_resolver("literal $${VAR} here", resolver_from(&vars));
        assert_eq!(r.content, "literal ${VAR} here");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_escape_preserves_content() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("$${UNTOUCHED:-default}", resolver_from(&vars));
        assert_eq!(r.content, "${UNTOUCHED:-default}");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_dollar_without_brace_is_literal() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("price is $5", resolver_from(&vars));
        assert_eq!(r.content, "price is $5");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_unterminated_reference_is_literal() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("broken ${VAR ref", resolver_from(&vars));
        assert_eq!(r.content, "broken ${VAR ref");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_empty_name_is_literal() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("${} test", resolver_from(&vars));
        assert_eq!(r.content, "${} test");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_invalid_name_start_is_literal() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("${123} test", resolver_from(&vars));
        assert_eq!(r.content, "${123} test");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_underscore_var_name() {
        let mut vars = HashMap::new();
        vars.insert("_MY_VAR_2", "ok");
        let r = interpolate_with_resolver("${_MY_VAR_2}", resolver_from(&vars));
        assert_eq!(r.content, "ok");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_var_in_toml_string() {
        let mut vars = HashMap::new();
        vars.insert("DB_HOST", "localhost");
        vars.insert("DB_PORT", "5432");
        let input = r#"
[coast]
name = "myapp"

[shared_services.postgres]
image = "postgres:16"
env = ["POSTGRES_HOST=${DB_HOST}", "POSTGRES_PORT=${DB_PORT}"]
"#;
        let r = interpolate_with_resolver(input, resolver_from(&vars));
        assert!(r.content.contains("POSTGRES_HOST=localhost"));
        assert!(r.content.contains("POSTGRES_PORT=5432"));
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_var_in_toml_value() {
        let mut vars = HashMap::new();
        vars.insert("PROJECT_NAME", "my-coast");
        let input = r#"
[coast]
name = "${PROJECT_NAME}"
compose = "./docker-compose.yml"
"#;
        let r = interpolate_with_resolver(input, resolver_from(&vars));
        assert!(r.content.contains("name = \"my-coast\""));
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_default_with_special_chars() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver(
            "url = \"${URL:-http://localhost:3000/api}\"",
            resolver_from(&vars),
        );
        assert_eq!(r.content, "url = \"http://localhost:3000/api\"");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_toml_comment_lines_are_substituted() {
        // Pre-parse interpolation applies to the entire text including comments.
        // This is by design — comments are inert to TOML parsing anyway.
        let mut vars = HashMap::new();
        vars.insert("VER", "16");
        let input = "# Using postgres ${VER}\nimage = \"postgres:${VER}\"";
        let r = interpolate_with_resolver(input, resolver_from(&vars));
        assert!(r.content.contains("postgres:16"));
        assert!(r.content.contains("# Using postgres 16"));
    }

    #[test]
    fn test_unicode_content_preserved() {
        let mut vars = HashMap::new();
        vars.insert("MSG", "hello");
        let r = interpolate_with_resolver("café: ${MSG} 日本語", resolver_from(&vars));
        assert_eq!(r.content, "café: hello 日本語");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_empty_input() {
        let vars: HashMap<&str, &str> = HashMap::new();
        let r = interpolate_with_resolver("", resolver_from(&vars));
        assert_eq!(r.content, "");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_only_variable() {
        let mut vars = HashMap::new();
        vars.insert("X", "value");
        let r = interpolate_with_resolver("${X}", resolver_from(&vars));
        assert_eq!(r.content, "value");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_var_value_with_special_toml_chars() {
        let mut vars = HashMap::new();
        vars.insert("SECRET", "p@ss=w0rd&special");
        let r = interpolate_with_resolver("val = \"${SECRET}\"", resolver_from(&vars));
        assert_eq!(r.content, "val = \"p@ss=w0rd&special\"");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn test_full_coastfile_interpolation() {
        let mut vars = HashMap::new();
        vars.insert("PROJECT", "demo");
        vars.insert("COMPOSE", "./docker-compose.yml");
        vars.insert("WEB_PORT", "3000");
        vars.insert("API_KEY_VAR", "MY_API_KEY");
        let input = r#"
[coast]
name = "${PROJECT}"
compose = "${COMPOSE}"

[ports]
web = ${WEB_PORT}

[secrets.api]
extractor = "env"
var = "${API_KEY_VAR}"
inject = "env:API_KEY"
"#;
        let r = interpolate_with_resolver(input, resolver_from(&vars));
        assert!(r.content.contains("name = \"demo\""));
        assert!(r.content.contains("compose = \"./docker-compose.yml\""));
        assert!(r.content.contains("web = 3000"));
        assert!(r.content.contains("var = \"MY_API_KEY\""));
        assert!(r.warnings.is_empty());
    }
}
