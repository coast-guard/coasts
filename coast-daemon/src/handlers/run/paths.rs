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

// --- host socat supervisor (Phase 27 / §24) ---
//
// Daemon-managed socat processes live on the host, one per
// `(project, service_name)` SSG service. Pidfiles + logs go under
// `<active_coast_home>/socats/`; the supervisor is in
// `handlers/ssg/host_socat.rs`.

/// Directory that holds `<project>--<service>.{pid,log}` files for
/// the Phase 27 host socat supervisor. Automatically follows
/// `COAST_HOME` — so `coastd` writes under `~/.coast/socats/` and
/// `coastd-dev` under `~/.coast-dev/socats/`.
pub(crate) fn host_socats_dir() -> PathBuf {
    active_coast_home().join("socats")
}

/// Return `(pidfile, logfile)` paths for the host socat backing
/// `(project, service_name)`. Uses `--` (double-dash) between the
/// project and service so a project name that contains a single
/// dash can't collide with a service name that starts with a dash.
pub(crate) fn host_socat_paths(project: &str, service: &str) -> (PathBuf, PathBuf) {
    let dir = host_socats_dir();
    let stem = format!("{project}--{service}");
    (
        dir.join(format!("{stem}.pid")),
        dir.join(format!("{stem}.log")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_caddy_pki_host_dir_uses_coast_home_env() {
        let _guard = crate::test_support::coast_home_env_lock();
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
        let _guard = crate::test_support::coast_home_env_lock();
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
