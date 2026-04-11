//! WebSocket telemetry stream for pantheon. Subscribers receive every
//! `LegionEvent` from the legion link, serialised as JSON.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::legion_link::LegionEvent;

use super::AppState;

pub async fn ws_telemetry(
    ws: WebSocketUpgrade,
    State(app): State<AppState>,
) -> impl IntoResponse {
    let rx = app.link.subscribe();
    ws.on_upgrade(move |socket| pump(socket, rx))
}

async fn pump(mut socket: WebSocket, mut rx: broadcast::Receiver<LegionEvent>) {
    loop {
        match rx.recv().await {
            Ok(evt) => {
                let payload = WsTelemetryFrame {
                    drone_id: evt.drone_id,
                    msg: evt.msg,
                };
                let json = match serde_json::to_string(&payload) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = ?e, "failed to serialise telemetry frame");
                        continue;
                    }
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    debug!("ws subscriber gone");
                    return;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(lagged = n, "ws subscriber lagged on broadcast");
            }
            Err(broadcast::error::RecvError::Closed) => {
                return;
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct WsTelemetryFrame {
    drone_id: String,
    msg: hivemind_protocol::LegionToOracle,
}
