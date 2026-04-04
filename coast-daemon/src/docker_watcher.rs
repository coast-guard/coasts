/// Polls Docker connectivity and emits `DockerStatusChanged` events
/// when the daemon comes up or goes down. Polls every 5 seconds.
use std::sync::Arc;

use tracing::{debug, warn};

use coast_core::protocol::CoastEvent;

use crate::server::AppState;

pub fn spawn_docker_watcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut was_connected: Option<bool> = None;

        loop {
            interval.tick().await;

            let mut connected = false;

            if let Some(docker) = state.docker.as_ref() {
                connected = docker.ping().await.is_ok();
            }

            if !connected {
                match coast_docker::host::connect_to_host_docker() {
                    Ok(docker) => {
                        if docker.ping().await.is_ok() {
                            state.docker.set(Some(docker));
                            connected = true;
                        } else {
                            state.docker.set(None);
                        }
                    }
                    Err(error) => {
                        state.docker.set(None);
                        debug!(error = %error, "docker reconnect attempt failed");
                        if state.docker.is_none() && was_connected != Some(false) {
                            warn!(error = %error, "Docker is unavailable while coastd is running");
                        }
                    }
                }
            }

            if was_connected != Some(connected) {
                debug!(connected, "docker status changed");
                state.emit_event(CoastEvent::DockerStatusChanged { connected });
                was_connected = Some(connected);
            }
        }
    });
}
