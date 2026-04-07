/// Coast Service — remote control plane for running coast containers.
///
/// Accepts connections from `coast-daemon` instances over HTTP, manages DinD
/// containers on the remote host, and maintains its own state database.
pub mod handlers;
pub mod port_manager;
pub mod reconcile;
pub mod server;
pub mod state;

use std::sync::Arc;

use tracing::info;

use state::ServiceState;

pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(run_service());
}

async fn run_service() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port: u16 = std::env::var("COAST_SERVICE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(31420);

    let state = Arc::new(ServiceState::new().expect("failed to initialize service state"));

    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            reconcile::reconcile_instances(&state).await;

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                reconcile::heal_running_instances(&state).await;
            }
        });
    }

    let app = server::router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap_or_else(|e| panic!("failed to bind to port {port}: {e}"));

    info!(port, "coast-service listening");

    axum::serve(listener, app)
        .await
        .expect("coast-service server error");
}
