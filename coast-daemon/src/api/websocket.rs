use std::ops::ControlFlow;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, warn};

use coast_core::protocol::CoastEvent;

use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/events", get(ws_handler))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Serialize a bus event and send it to the WebSocket client.
async fn forward_event_to_socket(
    socket: &mut WebSocket,
    event: Result<CoastEvent, RecvError>,
) -> ControlFlow<()> {
    match event {
        Ok(coast_event) => match serde_json::to_string(&coast_event) {
            Ok(json) => {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    return ControlFlow::Break(());
                }
            }
            Err(e) => {
                warn!("failed to serialize event: {e}");
            }
        },
        Err(RecvError::Lagged(n)) => {
            warn!("websocket client lagged, skipped {n} events");
        }
        Err(RecvError::Closed) => {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

/// Handle an inbound WebSocket message (Close, Ping/Pong, or ignore).
async fn handle_inbound_ws_message(
    socket: &mut WebSocket,
    msg: Option<Result<Message, axum::Error>>,
) -> ControlFlow<()> {
    match msg {
        Some(Ok(Message::Close(_))) | None => ControlFlow::Break(()),
        Some(Ok(Message::Ping(data))) => {
            if socket.send(Message::Pong(data)).await.is_err() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
        _ => ControlFlow::Continue(()),
    }
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.event_bus.subscribe();
    debug!("websocket client connected to event bus");

    loop {
        tokio::select! {
            event = rx.recv() => {
                if forward_event_to_socket(&mut socket, event).await.is_break() {
                    break;
                }
            }
            msg = socket.recv() => {
                if handle_inbound_ws_message(&mut socket, msg).await.is_break() {
                    break;
                }
            }
        }
    }

    debug!("websocket client disconnected");
}
