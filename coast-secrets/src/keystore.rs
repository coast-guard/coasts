/// Encrypted SQLite keystore for secret storage.
///
/// Secrets are encrypted with AES-256-GCM using the `orion` crate.
/// The encryption key is stored in macOS Keychain on mac or a
/// file with 0600 permissions on Linux.
use std::path::Path;

use chrono::{DateTime, Utc};
use orion::aead;
use rusqlite::{params, Connection};

use coast_core::error::{CoastError, Result};

/// A stored secret record from the keystore.
#[derive(Debug, Clone)]
pub struct StoredSecret {
    /// Coast image name this secret belongs to.
    pub coast_image: String,
    /// Secret name.
    pub secret_name: String,
    /// Decrypted secret value.
    pub value: Vec<u8>,
    /// How to inject: "env" or "file".
    pub inject_type: String,
    /// Injection target (var name or file path).
    pub inject_target: String,
    /// When the secret was extracted.
    pub extracted_at: DateTime<Utc>,
    /// Which extractor was used.
    pub extractor: String,
    /// Optional TTL in seconds.
    pub ttl_seconds: Option<i64>,
}

/// The encrypted keystore backed by SQLite.
pub struct Keystore {
    conn: Connection,
    key: aead::SecretKey,
}

impl Keystore {
    /// Open or create a keystore at the given path.
    ///
    /// If the keystore database doesn't exist, it will be created.
    /// The encryption key is loaded from the key path, or generated
    /// on first use.
    pub fn open(db_path: &Path, key_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path).map_err(|e| CoastError::Secret {
            message: format!("Failed to open keystore at {}: {e}", db_path.display()),
            source: Some(Box::new(e)),
        })?;

        Self::init_schema(&conn)?;

        let key = Self::load_or_create_key(key_path)?;

        Ok(Self { conn, key })
    }

    /// Open a keystore with an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| CoastError::Secret {
            message: format!("Failed to open in-memory keystore: {e}"),
            source: Some(Box::new(e)),
        })?;

        Self::init_schema(&conn)?;

        let key = aead::SecretKey::default();

        Ok(Self { conn, key })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS secrets (
                coast_image TEXT NOT NULL,
                secret_name TEXT NOT NULL,
                encrypted_value BLOB NOT NULL,
                inject_type TEXT NOT NULL,
                inject_target TEXT NOT NULL,
                extracted_at TEXT NOT NULL,
                extractor TEXT NOT NULL,
                ttl_seconds INTEGER,
                PRIMARY KEY (coast_image, secret_name)
            );",
        )
        .map_err(|e| CoastError::Secret {
            message: format!("Failed to initialize keystore schema: {e}"),
            source: Some(Box::new(e)),
        })?;
        Ok(())
    }

    fn load_or_create_key(key_path: &Path) -> Result<aead::SecretKey> {
        if key_path.exists() {
            let key_bytes = std::fs::read(key_path).map_err(|e| CoastError::Secret {
                message: format!(
                    "Failed to read encryption key from {}: {e}",
                    key_path.display()
                ),
                source: Some(Box::new(e)),
            })?;
            aead::SecretKey::from_slice(&key_bytes).map_err(|e| CoastError::Secret {
                message: format!("Invalid encryption key at {}: {e}", key_path.display()),
                source: Some(Box::new(e)),
            })
        } else {
            let key = aead::SecretKey::default();
            let key_bytes = key.unprotected_as_bytes();

            // Create parent directories if needed
            if let Some(parent) = key_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CoastError::Secret {
                    message: format!("Failed to create key directory {}: {e}", parent.display()),
                    source: Some(Box::new(e)),
                })?;
            }

            std::fs::write(key_path, key_bytes).map_err(|e| CoastError::Secret {
                message: format!(
                    "Failed to write encryption key to {}: {e}",
                    key_path.display()
                ),
                source: Some(Box::new(e)),
            })?;

            // Set file permissions to 0600 on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o600);
                std::fs::set_permissions(key_path, perms).map_err(|e| CoastError::Secret {
                    message: format!(
                        "Failed to set key file permissions on {}: {e}",
                        key_path.display()
                    ),
                    source: Some(Box::new(e)),
                })?;
            }

            Ok(key)
        }
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        aead::seal(&self.key, plaintext).map_err(|e| CoastError::Secret {
            message: format!("Encryption failed: {e}"),
            source: Some(Box::new(e)),
        })
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        aead::open(&self.key, ciphertext).map_err(|e| CoastError::Secret {
            message: format!("Decryption failed: {e}"),
            source: Some(Box::new(e)),
        })
    }

    /// Store a secret in the keystore.
    ///
    /// If a secret with the same image and name already exists, it is replaced.
    #[allow(clippy::too_many_arguments)]
    pub fn store_secret(
        &self,
        coast_image: &str,
        secret_name: &str,
        value: &[u8],
        inject_type: &str,
        inject_target: &str,
        extractor: &str,
        ttl_seconds: Option<i64>,
    ) -> Result<()> {
        let encrypted = self.encrypt(value)?;
        let now = Utc::now().to_rfc3339();

        self.conn
            .execute(
                "INSERT OR REPLACE INTO secrets
                 (coast_image, secret_name, encrypted_value, inject_type, inject_target,
                  extracted_at, extractor, ttl_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    coast_image,
                    secret_name,
                    encrypted,
                    inject_type,
                    inject_target,
                    now,
                    extractor,
                    ttl_seconds,
                ],
            )
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to store secret '{secret_name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(())
    }

    /// Retrieve a single secret by image and name.
    pub fn get_secret(&self, coast_image: &str, secret_name: &str) -> Result<Option<StoredSecret>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT coast_image, secret_name, encrypted_value, inject_type, inject_target,
                        extracted_at, extractor, ttl_seconds
                 FROM secrets WHERE coast_image = ?1 AND secret_name = ?2",
            )
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to prepare query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let result = stmt
            .query_row(params![coast_image, secret_name], |row| {
                Ok(RawSecret {
                    coast_image: row.get(0)?,
                    secret_name: row.get(1)?,
                    encrypted_value: row.get(2)?,
                    inject_type: row.get(3)?,
                    inject_target: row.get(4)?,
                    extracted_at: row.get(5)?,
                    extractor: row.get(6)?,
                    ttl_seconds: row.get(7)?,
                })
            })
            .optional()
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to query secret '{secret_name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        match result {
            Some(raw) => {
                let decrypted = self.decrypt(&raw.encrypted_value)?;
                Ok(Some(self.raw_to_stored(raw, decrypted)?))
            }
            None => Ok(None),
        }
    }

    /// Retrieve all secrets for a coast image.
    pub fn get_all_secrets(&self, coast_image: &str) -> Result<Vec<StoredSecret>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT coast_image, secret_name, encrypted_value, inject_type, inject_target,
                        extracted_at, extractor, ttl_seconds
                 FROM secrets WHERE coast_image = ?1",
            )
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to prepare query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![coast_image], |row| {
                Ok(RawSecret {
                    coast_image: row.get(0)?,
                    secret_name: row.get(1)?,
                    encrypted_value: row.get(2)?,
                    inject_type: row.get(3)?,
                    inject_target: row.get(4)?,
                    extracted_at: row.get(5)?,
                    extractor: row.get(6)?,
                    ttl_seconds: row.get(7)?,
                })
            })
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to query secrets: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut secrets = Vec::new();
        for row in rows {
            let raw = row.map_err(|e| CoastError::Secret {
                message: format!("Failed to read secret row: {e}"),
                source: Some(Box::new(e)),
            })?;
            let decrypted = self.decrypt(&raw.encrypted_value)?;
            secrets.push(self.raw_to_stored(raw, decrypted)?);
        }

        Ok(secrets)
    }

    /// Delete all secrets for a coast image.
    pub fn delete_secrets_for_image(&self, coast_image: &str) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "DELETE FROM secrets WHERE coast_image = ?1",
                params![coast_image],
            )
            .map_err(|e| CoastError::Secret {
                message: format!("Failed to delete secrets for image '{coast_image}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(count)
    }

    /// Get all secrets that have expired based on their TTL.
    pub fn get_expired_secrets(&self, coast_image: &str) -> Result<Vec<StoredSecret>> {
        let all = self.get_all_secrets(coast_image)?;
        let now = Utc::now();

        Ok(all
            .into_iter()
            .filter(|s| {
                if let Some(ttl) = s.ttl_seconds {
                    let expires_at = s.extracted_at + chrono::Duration::seconds(ttl);
                    now > expires_at
                } else {
                    false
                }
            })
            .collect())
    }

    fn raw_to_stored(&self, raw: RawSecret, decrypted: Vec<u8>) -> Result<StoredSecret> {
        let extracted_at = DateTime::parse_from_rfc3339(&raw.extracted_at)
            .map_err(|e| CoastError::Secret {
                message: format!("Invalid timestamp in keystore: {e}"),
                source: Some(Box::new(e)),
            })?
            .with_timezone(&Utc);

        Ok(StoredSecret {
            coast_image: raw.coast_image,
            secret_name: raw.secret_name,
            value: decrypted,
            inject_type: raw.inject_type,
            inject_target: raw.inject_target,
            extracted_at,
            extractor: raw.extractor,
            ttl_seconds: raw.ttl_seconds,
        })
    }
}

/// Internal raw row from the database before decryption.
struct RawSecret {
    coast_image: String,
    secret_name: String,
    encrypted_value: Vec<u8>,
    inject_type: String,
    inject_target: String,
    extracted_at: String,
    extractor: String,
    ttl_seconds: Option<i64>,
}

/// Extension trait to add optional() to rusqlite results.
trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret(
            "my-app",
            "api_key",
            b"secret123",
            "env",
            "API_KEY",
            "file",
            None,
        )
        .unwrap();

        let secret = ks.get_secret("my-app", "api_key").unwrap().unwrap();
        assert_eq!(secret.coast_image, "my-app");
        assert_eq!(secret.secret_name, "api_key");
        assert_eq!(secret.value, b"secret123");
        assert_eq!(secret.inject_type, "env");
        assert_eq!(secret.inject_target, "API_KEY");
        assert_eq!(secret.extractor, "file");
        assert!(secret.ttl_seconds.is_none());
    }

    #[test]
    fn test_encryption_roundtrip() {
        let ks = Keystore::open_in_memory().unwrap();

        let plaintext = b"this is a secret value with special chars: \x00\x01\xff";
        ks.store_secret("img", "sec", plaintext, "env", "VAR", "command", None)
            .unwrap();

        let secret = ks.get_secret("img", "sec").unwrap().unwrap();
        assert_eq!(secret.value, plaintext);
    }

    #[test]
    fn test_get_nonexistent_secret() {
        let ks = Keystore::open_in_memory().unwrap();
        let result = ks.get_secret("nope", "nope").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_replace_secret() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret("img", "key", b"old", "env", "VAR", "file", None)
            .unwrap();
        ks.store_secret("img", "key", b"new", "env", "VAR", "file", None)
            .unwrap();

        let secret = ks.get_secret("img", "key").unwrap().unwrap();
        assert_eq!(secret.value, b"new");
    }

    #[test]
    fn test_get_all_secrets() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret("img", "a", b"1", "env", "A", "file", None)
            .unwrap();
        ks.store_secret("img", "b", b"2", "file", "/b", "env", None)
            .unwrap();
        ks.store_secret("other", "c", b"3", "env", "C", "file", None)
            .unwrap();

        let secrets = ks.get_all_secrets("img").unwrap();
        assert_eq!(secrets.len(), 2);

        let other_secrets = ks.get_all_secrets("other").unwrap();
        assert_eq!(other_secrets.len(), 1);
    }

    #[test]
    fn test_delete_secrets_for_image() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret("img", "a", b"1", "env", "A", "file", None)
            .unwrap();
        ks.store_secret("img", "b", b"2", "env", "B", "file", None)
            .unwrap();
        ks.store_secret("other", "c", b"3", "env", "C", "file", None)
            .unwrap();

        let count = ks.delete_secrets_for_image("img").unwrap();
        assert_eq!(count, 2);

        let secrets = ks.get_all_secrets("img").unwrap();
        assert!(secrets.is_empty());

        let other = ks.get_all_secrets("other").unwrap();
        assert_eq!(other.len(), 1);
    }

    #[test]
    fn test_delete_nonexistent_image() {
        let ks = Keystore::open_in_memory().unwrap();
        let count = ks.delete_secrets_for_image("nope").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_ttl_expiry_detection() {
        let ks = Keystore::open_in_memory().unwrap();

        // Store a secret with a TTL of 1 second
        ks.store_secret("img", "short", b"val", "env", "V", "cmd", Some(1))
            .unwrap();

        // Store a secret with no TTL
        ks.store_secret("img", "forever", b"val", "env", "V", "cmd", None)
            .unwrap();

        // Store a secret with a long TTL
        ks.store_secret("img", "long", b"val", "env", "V", "cmd", Some(999999))
            .unwrap();

        // Wait for the short TTL to expire
        std::thread::sleep(std::time::Duration::from_secs(2));

        let expired = ks.get_expired_secrets("img").unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].secret_name, "short");
    }

    #[test]
    fn test_per_image_scoping() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret("app1", "key", b"val1", "env", "K", "file", None)
            .unwrap();
        ks.store_secret("app2", "key", b"val2", "env", "K", "file", None)
            .unwrap();

        let s1 = ks.get_secret("app1", "key").unwrap().unwrap();
        let s2 = ks.get_secret("app2", "key").unwrap().unwrap();
        assert_eq!(s1.value, b"val1");
        assert_eq!(s2.value, b"val2");
    }

    #[test]
    fn test_binary_secret() {
        let ks = Keystore::open_in_memory().unwrap();

        let binary_data: Vec<u8> = (0..=255).collect();
        ks.store_secret("img", "bin", &binary_data, "file", "/secret", "file", None)
            .unwrap();

        let secret = ks.get_secret("img", "bin").unwrap().unwrap();
        assert_eq!(secret.value, binary_data);
    }

    #[test]
    fn test_key_file_creation() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("keystore.db");
        let key_path = dir.path().join("keystore.key");

        // First open creates the key
        let _ks = Keystore::open(&db_path, &key_path).unwrap();
        assert!(key_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&key_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_key_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("keystore.db");
        let key_path = dir.path().join("keystore.key");

        // Store a secret
        {
            let ks = Keystore::open(&db_path, &key_path).unwrap();
            ks.store_secret("img", "key", b"secret", "env", "V", "file", None)
                .unwrap();
        }

        // Reopen and retrieve
        {
            let ks = Keystore::open(&db_path, &key_path).unwrap();
            let secret = ks.get_secret("img", "key").unwrap().unwrap();
            assert_eq!(secret.value, b"secret");
        }
    }

    #[test]
    fn test_with_ttl_seconds() {
        let ks = Keystore::open_in_memory().unwrap();

        ks.store_secret("img", "key", b"val", "env", "V", "cmd", Some(3600))
            .unwrap();

        let secret = ks.get_secret("img", "key").unwrap().unwrap();
        assert_eq!(secret.ttl_seconds, Some(3600));
    }
}
