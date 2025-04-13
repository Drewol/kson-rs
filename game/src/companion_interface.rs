use std::borrow::Cow;
use std::sync::{atomic::AtomicBool, Arc};

use crate::button_codes::UscButton;
use crate::config::GameConfig;
use crate::help::button_click_event;
use crate::{button_codes::UscInputEvent, song_provider, worker_service::WorkerService};
use futures::StreamExt;
use futures_util::SinkExt;
use log::{error, info, warn};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use specta::ts::ExportConfiguration;
use specta::{ts, Type};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, Type)]
#[serde(tag = "variant")]
pub enum GameState {
    None,
    TitleScreen,
    SongSelect {
        search_string: Cow<'static, str>,
        level_filter: u8,
        folder_filter_index: usize,
        sort_index: usize,
        filters: Vec<song_provider::SongFilterType>,
        sorts: Vec<song_provider::SongSort>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, Type)]
#[serde(tag = "variant", content = "v")]
pub enum ClientEvent {
    Invalid(Cow<'static, str>),
    Start,
    Back,
    SetSearch(Cow<'static, str>),
    SetLevelFilter(u8),
    SetSongFilterType(song_provider::SongFilterType),
    SetSongSort(song_provider::SongSort),
}

pub struct CompanionServer {
    event_bus: tokio::sync::broadcast::Sender<GameState>,
    pub active: Arc<AtomicBool>,
    _listener: poll_promise::Promise<()>,
}

async fn accept_connection(
    peer: SocketAddr,
    stream: TcpStream,
    event_proxy: winit::event_loop::EventLoopProxy<UscInputEvent>,
    new_events: tokio::sync::broadcast::Receiver<GameState>,
) {
    use tokio_tungstenite::tungstenite::Error;
    if let Err(e) = handle_connection(peer, stream, event_proxy, new_events).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
            err => error!("Error processing connection: {}", err),
        }
    }
}

async fn handle_connection(
    peer: SocketAddr,
    stream: TcpStream,
    event_proxy: winit::event_loop::EventLoopProxy<UscInputEvent>,
    mut new_events: tokio::sync::broadcast::Receiver<GameState>,
) -> tokio_tungstenite::tungstenite::Result<()> {
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Failed to accept");

    info!("New WebSocket connection: {}", peer);

    let (mut tx, mut rx) = ws_stream.split();
    let a = async {
        while let Ok(e) = new_events.recv().await {
            let res = tx
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&e).expect("Failed to serialize GameState"),
                ))
                .await;
            if res.is_err() {
                break;
            }
        }
    };

    let b = async {
        while let Some(Ok(e)) = rx.next().await {
            let tokio_tungstenite::tungstenite::Message::Text(data) = e else {
                continue;
            };
            let e: ClientEvent =
                serde_json::from_str(&data).unwrap_or(ClientEvent::Invalid(data.into()));
            if let ClientEvent::Invalid(m) = &e {
                warn!("Companion server got an invalid message: {}", m);
            }

            let events = match e {
                ClientEvent::Start => button_click_event(UscButton::Start),
                ClientEvent::Back => button_click_event(UscButton::Back),
                e => vec![UscInputEvent::ClientEvent(e)],
            };

            if events
                .into_iter()
                .map(|e| event_proxy.send_event(e))
                .any(|e| e.is_err())
            {
                break;
            }
        }
    };

    tokio::join!(a, b);

    Ok(())
}

impl CompanionServer {
    pub fn new(event_proxy: winit::event_loop::EventLoopProxy<UscInputEvent>) -> Self {
        let (event_bus, _) = tokio::sync::broadcast::channel(8);
        let client_bus: tokio::sync::broadcast::Sender<GameState> = event_bus.clone();

        #[cfg(not(target_os = "android"))]
        let _listener = start_listener(event_proxy, client_bus);

        #[cfg(target_os = "android")]
        let _listener = poll_promise::Promise::from_ready(());

        Self {
            event_bus,
            active: Arc::new(AtomicBool::new(false)),
            _listener,
        }
    }

    pub fn send_state(&self, state: GameState) {
        _ = self.event_bus.send(state);
    }
}

fn start_listener(
    event_proxy: winit::event_loop::EventLoopProxy<UscInputEvent>,
    client_bus: tokio::sync::broadcast::Sender<GameState>,
) -> poll_promise::Promise<()> {
    let _listener = if let Some(addr) = GameConfig::get().companion_address.as_ref() {
        let addr = addr.clone();
        poll_promise::Promise::spawn_async(async move {
            let listener = TcpListener::bind(&addr)
                .await
                .expect("Can't start companion server");
            info!("Companion server listening on {}", &addr);
            while let Ok((stream, _)) = listener.accept().await {
                let peer = stream
                    .peer_addr()
                    .expect("connected streams should have a peer address");
                info!("Peer address: {}", peer);

                #[cfg(not(target_os = "macos"))]
                {
                    tokio::spawn(accept_connection(
                        peer,
                        stream,
                        event_proxy.clone(),
                        client_bus.subscribe(),
                    ));
                }
            }
        })
    } else {
        poll_promise::Promise::from_ready(())
    };
    _listener
}

impl WorkerService for CompanionServer {
    fn update(&mut self) {
        self.active.store(
            self.event_bus.receiver_count() > 0,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

// Just output schema to stdout
pub fn print_schema() -> Vec<(&'static str, String)> {
    let server = schema_for!(GameState);
    let client = schema_for!(ClientEvent);
    vec![
        (
            "server.json",
            serde_json::to_string_pretty(&server).unwrap(),
        ),
        (
            "client.json",
            serde_json::to_string_pretty(&client).unwrap(),
        ),
    ]
}

pub fn print_ts(p: &str) {
    let config = ExportConfiguration::default()
        .bigint(ts::BigIntExportBehavior::Number)
        .export_by_default(Some(true));
    _ = specta::export::ts_with_cfg(p, &config);
}
