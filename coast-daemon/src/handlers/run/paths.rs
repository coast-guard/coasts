use std::path::PathBuf;

use coast_core::artifact::coast_home;

pub(super) const SHARED_CADDY_PKI_CONTAINER_PATH: &str = "/coast-caddy-pki";

fn fallback_coast_home() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".coast")
}

pub(super) fn active_coast_home() -> PathBuf {
    coast_home().unwrap_or_else(|_| fallback_coast_home())
}

pub(super) fn image_cache_dir() -> PathBuf {
    active_coast_home().join("image-cache")
}

pub(crate) fn project_images_dir(project: &str) -> PathBuf {
    active_coast_home().join("images").join(project)
}

pub(super) fn override_dir(project: &str, instance_name: &str) -> PathBuf {
    active_coast_home()
        .join("overrides")
        .join(project)
        .join(instance_name)
}

pub(super) fn shared_caddy_pki_host_dir() -> PathBuf {
    active_coast_home().join("caddy").join("pki")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn test_shared_caddy_pki_host_dir_uses_coast_home_env() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", "/tmp/coast-dev-test-home");
        }

        let path = shared_caddy_pki_host_dir();
        assert_eq!(path, PathBuf::from("/tmp/coast-dev-test-home/caddy/pki"));

        match prev {
            Some(value) => unsafe { std::env::set_var("COAST_HOME", value) },
            None => unsafe { std::env::remove_var("COAST_HOME") },
        }
    }

    #[test]
    fn test_shared_caddy_pki_host_dir_differs_for_distinct_install_homes() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let prev = std::env::var_os("COAST_HOME");

        unsafe {
            std::env::set_var("COAST_HOME", "/tmp/coast-prod-home");
        }
        let prod_path = shared_caddy_pki_host_dir();

        unsafe {
            std::env::set_var("COAST_HOME", "/tmp/coast-dev-home");
        }
        let dev_path = shared_caddy_pki_host_dir();

        assert_ne!(prod_path, dev_path);
        assert_eq!(prod_path, PathBuf::from("/tmp/coast-prod-home/caddy/pki"));
        assert_eq!(dev_path, PathBuf::from("/tmp/coast-dev-home/caddy/pki"));

        match prev {
            Some(value) => unsafe { std::env::set_var("COAST_HOME", value) },
            None => unsafe { std::env::remove_var("COAST_HOME") },
        }
    }
}
