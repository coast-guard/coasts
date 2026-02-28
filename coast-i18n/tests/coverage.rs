use serde_json::{Map, Value};
use std::collections::BTreeSet;

fn load_locale(name: &str) -> Map<String, Value> {
    let path = format!("{}/locales/{}.json", env!("CARGO_MANIFEST_DIR"), name);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {}: {}", path, e))
}

const LOCALES: &[&str] = &["en", "zh", "ja", "ko", "ru", "pt", "es"];

#[test]
fn key_parity_all_en_keys_present_in_every_locale() {
    let en = load_locale("en");
    let en_keys: BTreeSet<&str> = en.keys().map(|k| k.as_str()).collect();

    for &locale in LOCALES.iter().filter(|&&l| l != "en") {
        let other = load_locale(locale);
        let other_keys: BTreeSet<&str> = other.keys().map(|k| k.as_str()).collect();

        let missing: Vec<&&str> = en_keys.difference(&other_keys).collect();
        assert!(
            missing.is_empty(),
            "locale '{}' is missing keys present in en.json: {:?}",
            locale,
            missing,
        );
    }
}

#[test]
fn no_orphan_keys_in_any_locale() {
    let en = load_locale("en");
    let en_keys: BTreeSet<&str> = en.keys().map(|k| k.as_str()).collect();

    for &locale in LOCALES.iter().filter(|&&l| l != "en") {
        let other = load_locale(locale);
        let other_keys: BTreeSet<&str> = other.keys().map(|k| k.as_str()).collect();

        let orphans: Vec<&&str> = other_keys.difference(&en_keys).collect();
        assert!(
            orphans.is_empty(),
            "locale '{}' has keys not in en.json (orphans): {:?}",
            locale,
            orphans,
        );
    }
}

#[test]
fn all_coast_error_variants_have_i18n_keys() {
    let en = load_locale("en");

    let expected_keys = [
        ("CoastfileParse", "error.coastfile_parse"),
        ("Docker", "error.docker"),
        ("Git", "error.git"),
        ("Secret", "error.secret"),
        ("State", "error.state"),
        ("Port", "error.port"),
        ("Io", "error.io"),
        ("Artifact", "error.artifact"),
        ("Volume", "error.volume"),
        ("InstanceNotFound", "error.instance_not_found"),
        ("InstanceAlreadyExists", "error.instance_already_exists"),
        ("DanglingContainerDetected", "error.dangling_container"),
        ("RuntimeUnavailable", "error.runtime_unavailable"),
        ("Protocol", "error.protocol"),
    ];

    let mut missing = Vec::new();
    for (variant, key) in &expected_keys {
        if !en.contains_key(*key) {
            missing.push(format!("{} -> {}", variant, key));
        }
    }

    assert!(
        missing.is_empty(),
        "en.json is missing i18n keys for CoastError variants: {:?}",
        missing,
    );
}
