/// Bare process service supervisor for coast instances.
///
/// Generates shell scripts that manage bare processes inside a DinD
/// container as an alternative to docker-compose. Each service gets a
/// wrapper script with log redirection, PID tracking, and configurable
/// restart policy.
use coast_core::types::{BareServiceConfig, RestartPolicy};
use coast_docker::runtime::Runtime;

pub const SUPERVISOR_DIR: &str = "/coast-supervisor";
pub const LOG_DIR: &str = "/var/log/coast-services";
pub const CACHE_DIR: &str = "/coast-cache";

/// Check if a container has the supervisor directory, indicating it uses bare services.
pub async fn has_bare_services(docker: &bollard::Docker, container_id: &str) -> bool {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    match runtime
        .exec_in_coast(container_id, &["test", "-d", SUPERVISOR_DIR])
        .await
    {
        Ok(r) => r.success(),
        Err(_) => false,
    }
}

/// Generate a per-service wrapper script.
///
/// The script:
/// 1. Redirects stdout/stderr to a log file
/// 2. Writes a PID file
/// 3. Optionally restarts on exit per the restart policy
fn service_wrapper_script(svc: &BareServiceConfig) -> String {
    let name = &svc.name;
    let command = &svc.command;
    let pid_file = format!("{SUPERVISOR_DIR}/{name}.pid");
    let log_file = format!("{LOG_DIR}/{name}.log");

    let restart_loop = match svc.restart {
        RestartPolicy::No => format!(
            r#"cd /workspace
{command} >> {log_file} 2>&1 &
SVC_PID=$!
echo $SVC_PID > {pid_file}
wait $SVC_PID"#
        ),
        RestartPolicy::OnFailure => format!(
            r#"RETRIES=0
BACKOFF=1
while true; do
  cd /workspace
  START_TS=$(date +%s)
  {command} >> {log_file} 2>&1 &
  SVC_PID=$!
  echo $SVC_PID > {pid_file}
  wait $SVC_PID
  EXIT_CODE=$?
  if [ $EXIT_CODE -eq 0 ]; then
    break
  fi
  ELAPSED=$(( $(date +%s) - START_TS ))
  if [ $ELAPSED -gt 30 ]; then
    RETRIES=0
    BACKOFF=1
  fi
  RETRIES=$(( RETRIES + 1 ))
  if [ $RETRIES -ge 10 ]; then
    echo "[coast-supervisor] {name} crashed 10 times, giving up" >> {log_file}
    break
  fi
  echo "[coast-supervisor] {name} exited with code $EXIT_CODE, retry $RETRIES in ${{BACKOFF}}s..." >> {log_file}
  sleep $BACKOFF
  BACKOFF=$(( BACKOFF * 2 ))
  if [ $BACKOFF -gt 30 ]; then BACKOFF=30; fi
done"#
        ),
        RestartPolicy::Always => format!(
            r#"RETRIES=0
BACKOFF=1
while true; do
  cd /workspace
  START_TS=$(date +%s)
  {command} >> {log_file} 2>&1 &
  SVC_PID=$!
  echo $SVC_PID > {pid_file}
  wait $SVC_PID
  EXIT_CODE=$?
  ELAPSED=$(( $(date +%s) - START_TS ))
  if [ $ELAPSED -gt 30 ]; then
    RETRIES=0
    BACKOFF=1
  fi
  RETRIES=$(( RETRIES + 1 ))
  if [ $RETRIES -ge 10 ]; then
    echo "[coast-supervisor] {name} crashed 10 times, giving up" >> {log_file}
    break
  fi
  echo "[coast-supervisor] {name} exited with code $EXIT_CODE, retry $RETRIES in ${{BACKOFF}}s..." >> {log_file}
  sleep $BACKOFF
  BACKOFF=$(( BACKOFF * 2 ))
  if [ $BACKOFF -gt 30 ]; then BACKOFF=30; fi
done"#
        ),
    };

    let port_guard = if let Some(port) = svc.port {
        format!(
            "# Kill anything holding our port before starting\n\
             fuser -k {port}/tcp 2>/dev/null; sleep 0.2\n"
        )
    } else {
        String::new()
    };

    format!(
        "#!/bin/sh\n\
         # coast-supervisor wrapper for service '{name}'\n\
         # Guard: exit if another wrapper is already running this service\n\
         if [ -f {pid_file} ]; then\n\
         \x20 OLD_PID=$(cat {pid_file} 2>/dev/null)\n\
         \x20 if [ -n \"$OLD_PID\" ] && kill -0 \"$OLD_PID\" 2>/dev/null; then\n\
         \x20   exit 0\n\
         \x20 fi\n\
         fi\n\
         {port_guard}\
         {restart_loop}\n"
    )
}

/// Generate the start-all script that launches every service wrapper.
fn start_all_script(services: &[BareServiceConfig]) -> String {
    let mut script = format!(
        "#!/bin/sh\n\
         # coast-supervisor: start all bare services\n\
         mkdir -p {LOG_DIR}\n"
    );
    for svc in services {
        script.push_str(&format!(
            "sh {SUPERVISOR_DIR}/{name}.sh &\n\
             echo \"[coast-supervisor] started {name} (wrapper PID $!)\"\n",
            name = svc.name
        ));
    }
    script.push_str("echo \"[coast-supervisor] all services started\"\n");
    script
}

/// Generate the stop-all script that sends SIGTERM to each service.
fn stop_all_script(services: &[BareServiceConfig]) -> String {
    let mut script = "#!/bin/sh\n\
         # coast-supervisor: stop all bare services\n"
        .to_string();
    for svc in services {
        script.push_str(&format!(
            "if [ -f {SUPERVISOR_DIR}/{name}.pid ]; then\n\
             \x20 PID=$(cat {SUPERVISOR_DIR}/{name}.pid 2>/dev/null)\n\
             \x20 if [ -n \"$PID\" ] && kill -0 \"$PID\" 2>/dev/null; then\n\
             \x20   kill -- -\"$PID\" 2>/dev/null || kill \"$PID\" 2>/dev/null\n\
             \x20   sleep 1\n\
             \x20   kill -0 \"$PID\" 2>/dev/null && kill -9 -- -\"$PID\" 2>/dev/null || true\n\
             \x20   echo \"[coast-supervisor] stopped {name} (PID $PID)\"\n\
             \x20 fi\n\
             \x20 rm -f {SUPERVISOR_DIR}/{name}.pid\n\
             fi\n",
            name = svc.name
        ));
    }
    // Also kill any remaining wrapper shells and orphaned node processes
    script.push_str("pkill -f 'coast-supervisor wrapper' 2>/dev/null || true\n");
    script.push_str("echo \"[coast-supervisor] all services stopped\"\n");
    script
}

/// Generate the ps script that checks PID liveness and outputs JSON.
fn ps_script(services: &[BareServiceConfig]) -> String {
    let mut script = "#!/bin/sh\n# coast-supervisor: status of all bare services\n".to_string();
    for svc in services {
        script.push_str(&format!(
            "if [ -f {SUPERVISOR_DIR}/{name}.pid ]; then\n\
             \x20 PID=$(cat {SUPERVISOR_DIR}/{name}.pid 2>/dev/null)\n\
             \x20 if [ -n \"$PID\" ] && kill -0 \"$PID\" 2>/dev/null; then\n\
             \x20   echo '{{\"Service\":\"{name}\",\"State\":\"running\"}}'\n\
             \x20 else\n\
             \x20   echo '{{\"Service\":\"{name}\",\"State\":\"exited\"}}'\n\
             \x20 fi\n\
             else\n\
             \x20 echo '{{\"Service\":\"{name}\",\"State\":\"not started\"}}'\n\
             fi\n",
            name = svc.name
        ));
    }
    script
}

/// Build the complete `docker exec` command that writes all supervisor
/// scripts into the container and starts the services.
///
/// Returns a shell one-liner suitable for `exec_in_coast(container_id, &["sh", "-c", &cmd])`.
pub fn generate_setup_and_start_command(services: &[BareServiceConfig]) -> String {
    let mut parts: Vec<String> = vec![format!("mkdir -p {SUPERVISOR_DIR} {LOG_DIR}")];

    for svc in services {
        let wrapper = service_wrapper_script(svc);
        let escaped = shell_escape(&wrapper);
        parts.push(format!(
            "printf '%s' {escaped} > {SUPERVISOR_DIR}/{name}.sh && chmod +x {SUPERVISOR_DIR}/{name}.sh",
            name = svc.name
        ));
    }

    let start_all = start_all_script(services);
    let escaped_start = shell_escape(&start_all);
    parts.push(format!(
        "printf '%s' {escaped_start} > {SUPERVISOR_DIR}/start-all.sh && chmod +x {SUPERVISOR_DIR}/start-all.sh"
    ));

    let stop_all = stop_all_script(services);
    let escaped_stop = shell_escape(&stop_all);
    parts.push(format!(
        "printf '%s' {escaped_stop} > {SUPERVISOR_DIR}/stop-all.sh && chmod +x {SUPERVISOR_DIR}/stop-all.sh"
    ));

    let ps = ps_script(services);
    let escaped_ps = shell_escape(&ps);
    parts.push(format!(
        "printf '%s' {escaped_ps} > {SUPERVISOR_DIR}/ps.sh && chmod +x {SUPERVISOR_DIR}/ps.sh"
    ));

    // Run install steps (fail-fast) before starting services
    for svc in services {
        for cmd in &svc.install {
            let install_log = format!("{LOG_DIR}/{}.install.log", svc.name);
            parts.push(format!("cd /workspace && ({cmd}) >> {install_log} 2>&1"));
        }
    }

    parts.push(format!("sh {SUPERVISOR_DIR}/start-all.sh"));

    parts.join(" && ")
}

/// Save cached directories from /workspace to the persistent cache.
/// Uses cp -al (hardlinks) for near-instant copies without data duplication.
pub fn generate_cache_save_command(services: &[BareServiceConfig]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for svc in services {
        for dir in &svc.cache {
            let cache_dest = format!("{CACHE_DIR}/{}/{dir}", svc.name);
            parts.push(format!(
                "if [ -d '/workspace/{dir}' ]; then \
                   rm -rf '{cache_dest}' && \
                   mkdir -p '$(dirname \"{cache_dest}\")' && \
                   cp -al '/workspace/{dir}' '{cache_dest}' 2>/dev/null || \
                   cp -a '/workspace/{dir}' '{cache_dest}'; \
                 fi"
            ));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// Restore cached directories into /workspace via hardlink copy.
/// Each worktree gets its own independent copy (hardlinks share data on disk
/// but allow independent modifications like Vite's .vite/deps rebuild).
pub fn generate_cache_restore_command(services: &[BareServiceConfig]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for svc in services {
        for dir in &svc.cache {
            let cache_src = format!("{CACHE_DIR}/{}/{dir}", svc.name);
            parts.push(format!(
                "if [ -d '{cache_src}' ] && [ ! -e '/workspace/{dir}' ]; then \
                   mkdir -p '$(dirname \"/workspace/{dir}\")' && \
                   cp -al '{cache_src}' '/workspace/{dir}' 2>/dev/null || \
                   cp -a '{cache_src}' '/workspace/{dir}'; \
                   rm -rf '/workspace/{dir}/.vite' 2>/dev/null; \
                 fi"
            ));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("mkdir -p {CACHE_DIR}; {}", parts.join("; ")))
    }
}

/// Build the install-then-start command (for assign/branch-switch).
///
/// Restores cached directories first, then runs install steps, saves
/// the cache, and starts services.
pub fn generate_install_and_start_command(services: &[BareServiceConfig]) -> String {
    let mut parts: Vec<String> = vec![format!("mkdir -p {LOG_DIR}")];
    if let Some(restore) = generate_cache_restore_command(services) {
        parts.push(restore);
    }
    for svc in services {
        for cmd in &svc.install {
            let install_log = format!("{LOG_DIR}/{}.install.log", svc.name);
            parts.push(format!("cd /workspace && ({cmd}) >> {install_log} 2>&1"));
        }
    }
    if let Some(save) = generate_cache_save_command(services) {
        parts.push(save);
    }
    parts.push(format!("sh {SUPERVISOR_DIR}/start-all.sh"));
    parts.join(" && ")
}

/// Build the stop command.
pub fn generate_stop_command() -> String {
    format!("sh {SUPERVISOR_DIR}/stop-all.sh")
}

/// Build the start command (re-start after stop).
pub fn generate_start_command() -> String {
    format!("sh {SUPERVISOR_DIR}/start-all.sh")
}

/// Build the ps command.
pub fn generate_ps_command() -> String {
    format!("sh {SUPERVISOR_DIR}/ps.sh")
}

/// Build the logs tail command with compose-style service name prefixes.
///
/// Output format matches Docker Compose: `<service>  | <log line>`
/// so the frontend's existing service filter works without changes.
pub fn generate_logs_command(
    service: Option<&str>,
    tail: Option<u32>,
    tail_all: bool,
    follow: bool,
) -> String {
    let tail_args = if tail_all {
        "-n +1".to_string()
    } else if let Some(n) = tail {
        format!("-n {n}")
    } else {
        "-n 200".to_string()
    };

    let follow_flag = if follow { " -f" } else { "" };

    match service {
        Some(name) => {
            // Use awk with fflush() for unbuffered line-by-line output.
            // BusyBox sed buffers in pipes, causing streaming delays.
            format!(
                "tail {tail_args}{follow_flag} {LOG_DIR}/{name}.log | \
                 awk '{{print \"{name}  | \" $0; fflush()}}'"
            )
        }
        None => {
            // For all services: launch a background tail|awk per log file, then wait.
            // Each file's output is prefixed with the service name derived from the filename.
            // Skip *.install.log files (install output, not service runtime logs).
            // awk with fflush() ensures unbuffered line-by-line streaming.
            format!(
                "for f in {LOG_DIR}/*.log; do \
                 [ -f \"$f\" ] || continue; \
                 case \"$f\" in *.install.log) continue;; esac; \
                 svc=$(basename \"$f\" .log); \
                 tail {tail_args}{follow_flag} \"$f\" | \
                 awk -v svc=\"$svc\" '{{print svc \"  | \" $0; fflush()}}' & \
                 done; wait"
            )
        }
    }
}

fn shell_escape(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_svc(name: &str, command: &str, restart: RestartPolicy) -> BareServiceConfig {
        BareServiceConfig {
            name: name.to_string(),
            command: command.to_string(),
            port: None,
            restart,
            install: vec![],
            cache: vec![],
        }
    }

    #[test]
    fn test_service_wrapper_no_restart() {
        let svc = test_svc("web", "npm run dev", RestartPolicy::No);
        let script = service_wrapper_script(&svc);
        assert!(script.contains("npm run dev"));
        assert!(script.contains("/coast-supervisor/web.pid"));
        assert!(script.contains("/var/log/coast-services/web.log"));
        assert!(!script.contains("while true"));
    }

    #[test]
    fn test_service_wrapper_on_failure() {
        let svc = test_svc("api", "go run .", RestartPolicy::OnFailure);
        let script = service_wrapper_script(&svc);
        assert!(script.contains("while true"));
        assert!(script.contains("if [ $EXIT_CODE -eq 0 ]"));
        assert!(script.contains("break"));
        assert!(script.contains("BACKOFF"));
        assert!(script.contains("RETRIES"));
        assert!(script.contains("crashed 10 times"));
    }

    #[test]
    fn test_service_wrapper_always() {
        let svc = test_svc("worker", "python worker.py", RestartPolicy::Always);
        let script = service_wrapper_script(&svc);
        assert!(script.contains("while true"));
        assert!(script.contains("BACKOFF"));
        assert!(script.contains("RETRIES"));
        assert!(script.contains("crashed 10 times"));
    }

    #[test]
    fn test_start_all_script() {
        let services = vec![
            test_svc("web", "npm run dev", RestartPolicy::No),
            test_svc("worker", "npm run worker", RestartPolicy::Always),
        ];
        let script = start_all_script(&services);
        assert!(script.contains("web.sh"));
        assert!(script.contains("worker.sh"));
        assert!(script.contains("all services started"));
    }

    #[test]
    fn test_stop_all_script() {
        let services = vec![test_svc("web", "npm run dev", RestartPolicy::No)];
        let script = stop_all_script(&services);
        assert!(script.contains("web.pid"));
        assert!(script.contains("kill"));
        assert!(script.contains("all services stopped"));
    }

    #[test]
    fn test_ps_script() {
        let services = vec![test_svc("web", "npm run dev", RestartPolicy::No)];
        let script = ps_script(&services);
        assert!(script.contains("Service"));
        assert!(script.contains("State"));
        assert!(script.contains("running"));
        assert!(script.contains("exited"));
    }

    #[test]
    fn test_generate_setup_and_start_command() {
        let services = vec![test_svc("web", "npm run dev", RestartPolicy::No)];
        let cmd = generate_setup_and_start_command(&services);
        assert!(cmd.contains("mkdir -p"));
        assert!(cmd.contains("web.sh"));
        assert!(cmd.contains("start-all.sh"));
        assert!(cmd.contains("stop-all.sh"));
        assert!(cmd.contains("ps.sh"));
    }

    #[test]
    fn test_generate_logs_command_defaults() {
        let cmd = generate_logs_command(None, None, false, false);
        assert!(cmd.contains("tail -n 200"));
        assert!(cmd.contains("awk"));
        assert!(cmd.contains("fflush"));
        assert!(cmd.contains("for f in"));
    }

    #[test]
    fn test_generate_logs_command_service_filter() {
        let cmd = generate_logs_command(Some("web"), None, false, false);
        assert!(cmd.contains("tail -n 200"));
        assert!(cmd.contains("/web.log"));
        assert!(cmd.contains("awk"));
        assert!(cmd.contains("web  | "));
    }

    #[test]
    fn test_generate_logs_command_tail_follow() {
        let cmd = generate_logs_command(None, Some(50), false, true);
        assert!(cmd.contains("tail -n 50 -f"));
        assert!(cmd.contains("awk"));
        assert!(cmd.contains("wait"));
    }

    #[test]
    fn test_generate_logs_command_tail_all() {
        let cmd = generate_logs_command(None, None, true, true);
        assert!(cmd.contains("tail -n +1 -f"));
        assert!(cmd.contains("awk"));
    }

    #[test]
    fn test_generate_logs_command_single_service_follow() {
        let cmd = generate_logs_command(Some("api"), Some(100), false, true);
        assert!(cmd.contains("tail -n 100 -f"));
        assert!(cmd.contains("/api.log"));
        assert!(cmd.contains("awk"));
        assert!(cmd.contains("api  | "));
        assert!(!cmd.contains("for f in"));
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_generate_stop_command() {
        let cmd = generate_stop_command();
        assert_eq!(cmd, "sh /coast-supervisor/stop-all.sh");
    }

    #[test]
    fn test_generate_start_command() {
        let cmd = generate_start_command();
        assert_eq!(cmd, "sh /coast-supervisor/start-all.sh");
    }

    #[test]
    fn test_generate_ps_command() {
        let cmd = generate_ps_command();
        assert_eq!(cmd, "sh /coast-supervisor/ps.sh");
    }

    #[test]
    fn test_empty_services_setup_command() {
        let cmd = generate_setup_and_start_command(&[]);
        assert!(cmd.contains("mkdir -p"));
        assert!(cmd.contains("start-all.sh"));
    }

    #[test]
    fn test_empty_services_start_all_script() {
        let script = start_all_script(&[]);
        assert!(script.contains("all services started"));
        assert!(!script.contains(".sh &"));
    }

    #[test]
    fn test_setup_command_with_install_steps() {
        let svc = BareServiceConfig {
            name: "web".to_string(),
            command: "npm run dev".to_string(),
            port: None,
            restart: RestartPolicy::No,
            install: vec!["npm install".to_string(), "npm run build".to_string()],
            cache: vec![],
        };
        let cmd = generate_setup_and_start_command(&[svc]);
        assert!(cmd.contains("npm install"));
        assert!(cmd.contains("npm run build"));
        assert!(cmd.contains("web.install.log"));
        // The last segment should be "sh /coast-supervisor/start-all.sh"
        let segments: Vec<&str> = cmd.split(" && ").collect();
        assert_eq!(
            segments.last().unwrap().trim(),
            "sh /coast-supervisor/start-all.sh"
        );
        // Install steps should appear before the final start
        let install_segments: Vec<&&str> = segments
            .iter()
            .filter(|s| s.contains("install.log"))
            .collect();
        assert_eq!(install_segments.len(), 2);
    }

    #[test]
    fn test_setup_command_no_install_steps() {
        let svc = test_svc("web", "npm run dev", RestartPolicy::No);
        let cmd = generate_setup_and_start_command(&[svc]);
        assert!(!cmd.contains("install.log"));
        assert!(cmd.contains("start-all.sh"));
    }

    #[test]
    fn test_install_and_start_command() {
        let svc = BareServiceConfig {
            name: "api".to_string(),
            command: "go run .".to_string(),
            port: None,
            restart: RestartPolicy::No,
            install: vec!["go mod download".to_string()],
            cache: vec![],
        };
        let cmd = generate_install_and_start_command(&[svc]);
        assert!(cmd.contains("go mod download"));
        assert!(cmd.contains("api.install.log"));
        assert!(cmd.contains("start-all.sh"));
    }

    #[test]
    fn test_install_and_start_command_no_install() {
        let svc = test_svc("web", "npm run dev", RestartPolicy::No);
        let cmd = generate_install_and_start_command(&[svc]);
        assert!(!cmd.contains("install.log"));
        assert!(cmd.contains("start-all.sh"));
    }

    #[test]
    fn test_empty_services_stop_all_script() {
        let script = stop_all_script(&[]);
        assert!(script.contains("all services stopped"));
    }

    #[test]
    fn test_cache_restore_uses_hardlink_copy_not_symlink() {
        let svc = BareServiceConfig {
            name: "vite-web".to_string(),
            command: "npm run dev".to_string(),
            port: None,
            restart: RestartPolicy::No,
            install: vec![],
            cache: vec!["node_modules".to_string()],
        };
        let cmd = generate_cache_restore_command(&[svc]).unwrap();
        assert!(
            cmd.contains("cp -al"),
            "should use cp -al (hardlink copy), not symlink"
        );
        assert!(!cmd.contains("ln -sfn"), "should NOT use ln -sfn (symlink)");
        assert!(
            cmd.contains("rm -rf '/workspace/node_modules/.vite'"),
            "should clean stale Vite cache after restore"
        );
        assert!(
            cmd.contains("/coast-cache/vite-web/node_modules"),
            "cache source should be /coast-cache/<service>/<dir>"
        );
    }

    #[test]
    fn test_cache_save_command() {
        let svc = BareServiceConfig {
            name: "vite-web".to_string(),
            command: "npm run dev".to_string(),
            port: None,
            restart: RestartPolicy::No,
            install: vec![],
            cache: vec!["node_modules".to_string()],
        };
        let cmd = generate_cache_save_command(&[svc]).unwrap();
        assert!(cmd.contains("cp -al"), "should use cp -al for save");
        assert!(
            cmd.contains("/coast-cache/vite-web/node_modules"),
            "cache dest should be /coast-cache/<service>/<dir>"
        );
    }

    #[test]
    fn test_cache_commands_none_when_no_cache_dirs() {
        let svc = test_svc("web", "npm run dev", RestartPolicy::No);
        assert!(generate_cache_save_command(&[svc.clone()]).is_none());
        assert!(generate_cache_restore_command(&[svc]).is_none());
    }

    #[test]
    fn test_install_and_start_with_cache_includes_restore_and_save() {
        let svc = BareServiceConfig {
            name: "vite".to_string(),
            command: "npm run dev".to_string(),
            port: Some(3040),
            restart: RestartPolicy::OnFailure,
            install: vec!["npm install".to_string()],
            cache: vec!["node_modules".to_string()],
        };
        let cmd = generate_install_and_start_command(&[svc]);
        let segments: Vec<&str> = cmd.split(" && ").collect();

        let restore_idx = segments
            .iter()
            .position(|s| s.contains("coast-cache") && s.contains("cp -al"))
            .expect("must have cache restore step");
        let install_idx = segments
            .iter()
            .position(|s| s.contains("npm install"))
            .expect("must have install step");
        let save_idx = segments
            .iter()
            .rposition(|s| s.contains("coast-cache") && s.contains("cp -al"))
            .expect("must have cache save step");
        let start_idx = segments
            .iter()
            .position(|s| s.contains("start-all.sh"))
            .expect("must have start step");

        assert!(
            restore_idx < install_idx,
            "cache restore must come before install"
        );
        assert!(
            install_idx < save_idx,
            "install must come before cache save"
        );
        assert!(save_idx < start_idx, "cache save must come before start");
    }

    #[test]
    fn test_service_wrapper_script_has_pid_guard() {
        let svc = BareServiceConfig {
            name: "vite".to_string(),
            command: "npm run dev".to_string(),
            port: Some(3040),
            restart: RestartPolicy::OnFailure,
            install: vec![],
            cache: vec![],
        };
        let script = service_wrapper_script(&svc);
        assert!(
            script.contains(".pid"),
            "wrapper must check PID file for duplicate guard"
        );
        assert!(
            script.contains("kill -0"),
            "wrapper must check if old PID is alive"
        );
        assert!(
            script.contains("exit 0"),
            "wrapper must exit if already running"
        );
    }

    #[test]
    fn test_service_wrapper_script_port_cleanup() {
        let svc = BareServiceConfig {
            name: "vite".to_string(),
            command: "npm run dev".to_string(),
            port: Some(3040),
            restart: RestartPolicy::OnFailure,
            install: vec![],
            cache: vec![],
        };
        let script = service_wrapper_script(&svc);
        assert!(
            script.contains("fuser -k 3040/tcp"),
            "wrapper must kill processes holding the port before starting"
        );
    }

    #[test]
    fn test_service_wrapper_script_no_port_no_fuser() {
        let svc = test_svc("worker", "npm run worker", RestartPolicy::OnFailure);
        let script = service_wrapper_script(&svc);
        assert!(
            !script.contains("fuser"),
            "wrapper without port should not use fuser"
        );
    }

    #[test]
    fn test_stop_all_script_kills_child_processes() {
        let svc = test_svc("web", "npm run dev", RestartPolicy::OnFailure);
        let script = stop_all_script(&[svc]);
        assert!(
            script.contains("kill -- -"),
            "stop script must kill process group via kill -- -PID"
        );
        assert!(
            script.contains("kill -9 -- -"),
            "stop script must kill each child"
        );
    }
}
