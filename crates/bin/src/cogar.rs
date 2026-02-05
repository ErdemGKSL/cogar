//! Cogar - Unified game server with embedded frontend.

use axum::{
    extract::{ws::{WebSocket, WebSocketUpgrade}, ConnectInfo, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use tokio::sync::{broadcast, RwLock};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

// Embedded static assets from client/web
#[derive(RustEmbed)]
#[folder = "../client/web"]
struct Assets;

// Cache for generated skins list
static SKINS_LIST: OnceLock<String> = OnceLock::new();

/// Generate comma-separated list of available skins from embedded assets
fn generate_skins_list() -> String {
    let mut skins = Vec::new();
    
    // Iterate through all embedded assets
    for file_path in Assets::iter() {
        // Check if file is in skins/ directory
        if file_path.starts_with("skins/") {
            // Extract filename without extension
            if let Some(filename) = file_path.strip_prefix("skins/") {
                // Remove .png or .webp extension
                let skin_name = filename
                    .strip_suffix(".png")
                    .or_else(|| filename.strip_suffix(".webp"))
                    .unwrap_or(filename);
                
                // Avoid duplicates (same skin might have both .png and .webp)
                if !skins.contains(&skin_name.to_string()) {
                    skins.push(skin_name.to_string());
                }
            }
        }
    }
    
    // Sort alphabetically for consistency
    skins.sort();
    
    // Join with commas
    skins.join(",")
}

#[derive(Clone)]
struct AppState {
    game_state: Arc<RwLock<server::server::game::GameState>>,
    chat_tx: broadcast::Sender<server::ChatBroadcast>,
    lb_tx: broadcast::Sender<server::LeaderboardBroadcast>,
    world_tx: broadcast::Sender<server::WorldUpdateBroadcast>,
    targeted_tx: broadcast::Sender<server::TargetedMessage>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,server=debug")),
        )
        .init();

    info!("Native Ogar Server v{}", env!("CARGO_PKG_VERSION"));

    // Load server configuration
    let config = server::Config::load()?;
    info!("Loaded configuration");
    info!("  Port: {}", config.server.port);
    info!("  Border: {}x{}", config.border.width, config.border.height);
    info!("  Game mode: {}", config.server.gamemode);

    // Generate skins list at startup
    let skins_list = generate_skins_list();
    info!("Found {} skins: {}", skins_list.split(',').filter(|s| !s.is_empty()).count(), skins_list);
    SKINS_LIST.set(skins_list).ok();

    // Create broadcast channels
    let (chat_tx, _) = broadcast::channel::<server::ChatBroadcast>(100);
    let (lb_tx, _) = broadcast::channel::<server::LeaderboardBroadcast>(10);
    let (world_tx, _) = broadcast::channel::<server::WorldUpdateBroadcast>(5);
    let (targeted_tx, _) = broadcast::channel::<server::TargetedMessage>(100);

    // Create shared game state
    let game_state = Arc::new(RwLock::new(server::server::game::GameState::new(
        &config,
        chat_tx.clone(),
        lb_tx.clone(),
        world_tx.clone(),
        targeted_tx.clone(),
    )));

    // Start the game loop
    let game_loop_state = Arc::clone(&game_state);
    let tick_interval = config.server.tick_interval_ms;
    tokio::spawn(async move {
        server::server::game::run_game_loop(game_loop_state, tick_interval).await;
    });

    // Create app state
    let state = AppState {
        game_state,
        chat_tx,
        lb_tx,
        world_tx,
        targeted_tx,
    };

    // Build the axum router
    let app = Router::new()
        // WebSocket game endpoint
        .route("/game", get(websocket_handler))
        // Static file serving (index.html, CSS, WASM, etc.)
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/skinList.txt", get(serve_skins_txt))
        .fallback(static_handler)
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive())
        )
        .with_state(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    info!("Server running on http://{}", addr);
    info!("Game WebSocket endpoint: ws://{}/game", addr);

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}

/// Handle WebSocket connections for the game
async fn websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("WebSocket connection from {}", addr);
    
    ws.on_upgrade(move |socket| handle_websocket(socket, addr, state))
}

/// Handle individual WebSocket connections
async fn handle_websocket(
    socket: WebSocket,
    addr: SocketAddr,
    state: AppState,
) {
    info!("New game connection from {}", addr);

    // Subscribe to broadcast channels
    let chat_rx = state.chat_tx.subscribe();
    let lb_rx = state.lb_tx.subscribe();
    let world_rx = state.world_tx.subscribe();
    let targeted_rx = state.targeted_tx.subscribe();

    // Handle the connection using server logic
    if let Err(e) = handle_game_connection(
        socket,
        addr,
        state.game_state,
        chat_rx,
        lb_rx,
        world_rx,
        targeted_rx,
    ).await {
        error!("Connection error from {}: {}", addr, e);
    }
}

/// Adapt Axum WebSocket to work with server's game connection handler
async fn handle_game_connection(
    socket: WebSocket,
    addr: SocketAddr,
    game_state: Arc<RwLock<server::server::game::GameState>>,
    mut chat_rx: broadcast::Receiver<server::ChatBroadcast>,
    mut lb_rx: broadcast::Receiver<server::LeaderboardBroadcast>,
    mut world_rx: broadcast::Receiver<server::WorldUpdateBroadcast>,
    mut targeted_rx: broadcast::Receiver<server::TargetedMessage>,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    
    let (mut write, mut read) = socket.split();

    // Create client
    let client_id = {
        let mut state = game_state.write().await;
        state.add_client(addr)
    };

    // Track which nodes this client has seen (for delta updates)
    let mut client_nodes: HashSet<u32> = HashSet::new();

    // Message loop - handle both incoming messages and broadcasts
    loop {
        tokio::select! {
            // Handle incoming WebSocket messages
            msg = read.next() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Binary(data))) => {
                        let mut state = game_state.write().await;
                        if let Err(e) = state.handle_packet(client_id, &data) {
                            warn!("Packet error from {}: {}", addr, e);
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error from {}: {}", addr, e);
                        break;
                    }
                    None => {
                        break;
                    }
                    _ => {}
                }
            }
            // Handle chat broadcasts
            chat_msg = chat_rx.recv() => {
                if let Ok(chat) = chat_msg {
                    let packet = protocol::packets::build_chat_message(
                        chat.color,
                        &chat.name,
                        &chat.message,
                        chat.is_server,
                        false,
                        false,
                    );
                    let bytes = packet.finish();
                    if let Err(e) = send_binary(&mut write, bytes).await {
                        warn!("Failed to send chat to {}: {}", addr, e);
                        break;
                    }
                }
            }
            // Handle leaderboard broadcasts
            lb_msg = lb_rx.recv() => {
                if let Ok(lb) = lb_msg {
                    match lb.gamemode_id {
                        1 => {
                            let team_scores: Vec<f32> = lb.entries.iter()
                                .map(|e| e.score)
                                .collect();
                            let packet = protocol::packets::build_leaderboard_pie(&team_scores);
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send pie leaderboard to {}: {}", addr, e);
                                break;
                            }
                        }
                        _ => {
                            let entries: Vec<(bool, &str)> = lb.entries.iter()
                                .take(10)
                                .map(|e| (e.client_id == client_id, e.name.as_str()))
                                .collect();

                            let packet = protocol::packets::build_leaderboard_ffa(&entries);
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send ffa leaderboard to {}: {}", addr, e);
                                break;
                            }
                        }
                    }
                }
            }
            // Handle world update broadcasts
            world_msg = world_rx.recv() => {
                if let Ok(world) = world_msg {
                    let client_view = match world.client_data.get(&client_id) {
                        Some(v) => v,
                        None => continue,
                    };

                    let scale = client_view.scale.max(0.15);
                    let view_half_w = (1920.0 / scale) / 2.0;
                    let view_half_h = (1080.0 / scale) / 2.0;
                    let view_min_x = client_view.center_x - view_half_w;
                    let view_min_y = client_view.center_y - view_half_h;
                    let view_max_x = client_view.center_x + view_half_w;
                    let view_max_y = client_view.center_y + view_half_h;

                    let mut view_nodes: HashSet<u32> = HashSet::new();
                    for cell in &world.cells {
                        let margin = cell.size;
                        if cell.x + margin >= view_min_x
                            && cell.x - margin <= view_max_x
                            && cell.y + margin >= view_min_y
                            && cell.y - margin <= view_max_y
                        {
                            view_nodes.insert(cell.node_id);
                        }
                    }

                    for &cell_id in &client_view.cell_ids {
                        view_nodes.insert(cell_id);
                    }

                    // Force-include all minion cells (always visible to owner)
                    for &minion_id in &client_view.minion_ids {
                        for cell in &world.cells {
                            if cell.owner_id == Some(minion_id) {
                                view_nodes.insert(cell.node_id);
                            }
                        }
                    }

                    let mut add_nodes = Vec::new();
                    let mut upd_nodes = Vec::new();
                    let mut del_nodes = Vec::new();

                    for cell in &world.cells {
                        if view_nodes.contains(&cell.node_id) {
                            let is_new = !client_nodes.contains(&cell.node_id);

                            let update_cell = protocol::packets::UpdateCell {
                                node_id: cell.node_id,
                                x: cell.x as i32,
                                y: cell.y as i32,
                                size: cell.size as u16,
                                color: cell.color,
                                flags: protocol::packets::CellFlags {
                                    is_spiked: cell.cell_type == 2,
                                    is_player: true,
                                    has_skin: is_new && cell.skin.is_some(),
                                    has_name: is_new && cell.name.is_some(),
                                    is_agitated: false,
                                    is_ejected: cell.cell_type == 3,
                                    is_food: cell.cell_type == 1,
                                },
                                skin: if is_new { cell.skin.clone() } else { None },
                                name: if is_new { cell.name.clone() } else { None },
                            };

                            if is_new {
                                add_nodes.push(update_cell);
                            } else {
                                upd_nodes.push(update_cell);
                            }
                        }
                    }

                    for &node_id in &client_nodes {
                        if !view_nodes.contains(&node_id) {
                            del_nodes.push(node_id);
                        }
                    }

                    let eat_records: Vec<protocol::packets::EatRecord> = world.eaten.iter()
                        .filter(|(eaten_id, eater_id)| {
                            view_nodes.contains(eaten_id)
                                || view_nodes.contains(eater_id)
                                || client_nodes.contains(eaten_id)
                                || client_nodes.contains(eater_id)
                        })
                        .map(|&(eaten_id, eater_id)| protocol::packets::EatRecord { eaten_id, eater_id })
                        .collect();

                    client_nodes = view_nodes;

                    let packet = protocol::packets::build_update_nodes(
                        client_view.protocol,
                        client_view.scramble_id,
                        client_view.scramble_x,
                        client_view.scramble_y,
                        &add_nodes,
                        &upd_nodes,
                        &eat_records,
                        &del_nodes,
                    );
                    let bytes = packet.finish();

                    if let Err(e) = send_binary(&mut write, bytes).await {
                        warn!("Failed to send world update to {}: {}", addr, e);
                        break;
                    }
                }
            }
            // Handle targeted messages
            targeted_msg = targeted_rx.recv() => {
                if let Ok(msg) = targeted_msg {
                    if msg.client_id != client_id {
                        continue;
                    }

                    match msg.message {
                        server::TargetedMessageType::AddNode { node_id, scramble_id } => {
                            let packet = protocol::packets::build_add_node(node_id, scramble_id);
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send AddNode to {}: {}", addr, e);
                                break;
                            }
                        }
                        server::TargetedMessageType::ClearAll => {
                            let packet = protocol::packets::build_clear_all();
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send ClearAll to {}: {}", addr, e);
                                break;
                            }
                        }
                        server::TargetedMessageType::SetBorder { min_x, min_y, max_x, max_y, scramble_x, scramble_y, game_type, server_name } => {
                            let packet = protocol::packets::build_set_border(
                                min_x + scramble_x as f64,
                                min_y + scramble_y as f64,
                                max_x + scramble_x as f64,
                                max_y + scramble_y as f64,
                                game_type,
                                &server_name
                            );
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send SetBorder to {}: {}", addr, e);
                                break;
                            }
                        }
                        server::TargetedMessageType::ServerStat { json } => {
                            let packet = protocol::packets::build_server_stat(&json);
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send ServerStat to {}: {}", addr, e);
                                break;
                            }
                        }
                        server::TargetedMessageType::ChatMessage { name, color, message, is_server } => {
                            let packet = protocol::packets::build_chat_message(
                                color,
                                &name,
                                &message,
                                is_server,
                                false,
                                false,
                            );
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send ChatMessage to {}: {}", addr, e);
                                break;
                            }
                        }
                        server::TargetedMessageType::XrayData { player_cells, scramble_id, scramble_x, scramble_y } => {
                            let packet = protocol::packets::build_xray_data(
                                scramble_id,
                                scramble_x,
                                scramble_y,
                                &player_cells,
                            );
                            let bytes = packet.finish();
                            if let Err(e) = send_binary(&mut write, bytes).await {
                                warn!("Failed to send XrayData to {}: {}", addr, e);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Remove client
    {
        let mut state = game_state.write().await;
        state.remove_client(client_id);
    }

    Ok(())
}

async fn serve_index(headers: axum::http::HeaderMap) -> impl IntoResponse {
    let host = headers.get("host")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let scheme = headers.get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_ascii_lowercase())
        .map(|proto| if proto == "https" { "wss".to_string() } else { "ws".to_string() })
        .unwrap_or_else(|| "ws".to_string());

    serve_static_file_with_host("index.html".to_string(), host, Some(scheme)).await
}

/// Serve dynamically generated skins.txt
async fn serve_skins_txt() -> impl IntoResponse {
    let skins_list = SKINS_LIST.get().cloned().unwrap_or_default();
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(axum::body::Body::from(skins_list))
        .unwrap()
}

/// Handle static file requests
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/').to_string();
    
    // Handle empty path or root
    if path.is_empty() || path == "/" {
        return serve_static_file("index.html".to_string()).await;
    }
    
    serve_static_file(path).await
}

/// Serve a static file from embedded assets
async fn serve_static_file(path: String) -> impl IntoResponse {
    serve_static_file_with_host(path, None, None).await
}

/// Serve a static file from embedded assets with optional host injection
async fn serve_static_file_with_host(
    path: String,
    host: Option<String>,
    scheme: Option<String>,
) -> impl IntoResponse {
    match Assets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            
            // For index.html, inject connection URL
            let body = if path == "index.html" {
                if let Ok(content_str) = std::str::from_utf8(&content.data) {
                    // Auto-inject connection URL for cogar
                    let connection_url = if let (Some(host_header), Some(ws_scheme)) = (host, scheme) {
                        format!("{}://{}/game", ws_scheme, host_header.trim_end_matches('/'))
                    } else {
                        "/game".to_string()
                    };
                    
                    let injected_content = content_str.replace(
                        "// COGAR_CONNECTION_INJECT_POINT",
                        &format!("window.COGAR_CONNECTION = '{}'; // Auto-injected by cogar", connection_url)
                    );
                    
                    axum::body::Body::from(injected_content)
                } else {
                    axum::body::Body::from(content.data.to_vec())
                }
            } else {
                axum::body::Body::from(content.data.to_vec())
            };
            
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(body)
                .unwrap()
        }
        None => {
            warn!("Static file not found: {}", path);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap()
        }
    }
}

async fn send_binary(
    write: &mut futures_util::stream::SplitSink<WebSocket, axum::extract::ws::Message>,
    bytes: bytes::Bytes,
) -> anyhow::Result<()> {
    // Convert Bytes to the format axum expects (matching tokio-tungstenite behavior)
    write.send(axum::extract::ws::Message::Binary(bytes.to_vec().into())).await?;
    Ok(())
}