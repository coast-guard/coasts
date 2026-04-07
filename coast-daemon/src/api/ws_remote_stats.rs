use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use ts_rs::TS;

use crate::remote_stats::start_remote_stats_collector;
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct RemoteStatsParams {
    pub name: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/remote/stats/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<RemoteStatsParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let db = state.db.lock().await;
    let _entry = db
        .get_remote(&params.name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Remote '{}' not found", params.name),
            )
        })?;
    drop(db);

    let key = params.name.clone();

    if !state
        .remote_streaming_collectors
        .lock()
        .await
        .contains_key(&key)
    {
        start_remote_stats_collector(state.clone(), key.clone()).await;
    }

    Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)))
}

async fn replay_history(socket: &mut WebSocket, state: &AppState, key: &str) -> bool {
    let history = state.remote_streaming_history.lock().await;
    let Some(ring) = history.get(key) else {
        return true;
    };
    for val in ring.iter() {
        if socket
            .send(Message::Text(val.to_string().into()))
            .await
            .is_err()
        {
            return false;
        }
    }
    true
}

async fn stream_broadcast(
    socket: &mut WebSocket,
    rx: &mut broadcast::Receiver<serde_json::Value>,
    key: &str,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(val) => {
                        if socket.send(Message::Text(val.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(remote = %key, skipped = n, "remote stats WS lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(remote = %key, "remote stats WS connected");

    if !replay_history(&mut socket, &state, &key).await {
        return;
    }

    let mut rx = {
        let broadcasts = state.remote_streaming_broadcasts.lock().await;
        let Some(tx) = broadcasts.get(&key) else {
            let _ = socket
                .send(Message::Text("Stats collector not running".into()))
                .await;
            return;
        };
        tx.subscribe()
    };

    stream_broadcast(&mut socket, &mut rx, &key).await;
    debug!(remote = %key, "remote stats WS disconnected");
}
