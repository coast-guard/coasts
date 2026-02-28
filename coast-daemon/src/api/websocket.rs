use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tracing::{debug, warn};

use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/events", get(ws_handler))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.event_bus.subscribe();
    debug!("websocket client connected to event bus");

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(coast_event) => {
                        match serde_json::to_string(&coast_event) {
                            Ok(json) => {
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("failed to serialize event: {e}");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("websocket client lagged, skipped {n} events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    debug!("websocket client disconnected");
}
