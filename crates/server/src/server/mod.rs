//! Game server implementation.

use crate::config::Config;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{error, info, warn};

pub mod client;
pub mod game;

pub use game::{GameState, run_game_loop};

use protocol::Color;

/// A chat message to be broadcast to all clients.
#[derive(Debug, Clone)]
pub struct ChatBroadcast {
    /// Sender name (or "SERVER" for server messages).
    pub name: String,
    /// Message color.
    pub color: Color,
    /// Message text.
    pub message: String,
    /// Whether this is a server message.
    pub is_server: bool,
}

/// A leaderboard entry.
#[derive(Debug, Clone)]
pub struct LeaderboardEntry {
    /// Client ID.
    pub client_id: u32,
    /// Player name.
    pub name: String,
    /// Player score (total mass).
    pub score: f32,
}

/// Leaderboard update broadcast.
#[derive(Debug, Clone)]
pub struct LeaderboardBroadcast {
    /// Sorted list of leaderboard entries (highest score first).
    pub entries: Vec<LeaderboardEntry>,
    /// Active gamemode ID.
    pub gamemode_id: u32,
    /// Active gamemode name.
    pub gamemode_name: String,
}

/// Cell data for world updates.
#[derive(Debug, Clone)]
pub struct WorldCell {
    pub node_id: u32,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Color,
    pub cell_type: u8,
    pub name: Option<String>,
    pub skin: Option<String>,
    pub owner_id: Option<u32>,
}

/// World state update broadcast (sent every tick).
#[derive(Debug, Clone)]
pub struct WorldUpdateBroadcast {
    /// All cells in the world.
    pub cells: Vec<WorldCell>,
    /// Cells that were eaten this tick: (eaten_id, eater_id).
    pub eaten: Vec<(u32, u32)>,
    /// Cells that were removed this tick.
    pub removed: Vec<u32>,
    /// Per-client data (client_id -> (center_x, center_y, scale, cell_ids)).
    pub client_data: HashMap<u32, ClientViewData>,
}

/// Per-client view data.
#[derive(Debug, Clone)]
pub struct ClientViewData {
    pub center_x: f32,
    pub center_y: f32,
    pub scale: f32,
    pub cell_ids: Vec<u32>,
    pub minion_ids: Vec<u32>,
    pub protocol: u32,
    pub scramble_id: u32,
    pub scramble_x: i32,
    pub scramble_y: i32,
    pub name: String,
    pub skin: Option<String>,
}

/// A message targeted at a specific client.
#[derive(Debug, Clone)]
pub struct TargetedMessage {
    /// Target client ID.
    pub client_id: u32,
    /// The message type.
    pub message: TargetedMessageType,
}

/// Types of targeted messages.
#[derive(Debug, Clone)]
pub enum TargetedMessageType {
    /// AddNode packet - tells client it owns a cell.
    AddNode { node_id: u32, scramble_id: u32 },
    /// ClearAll packet - sent after handshake.
    ClearAll,
    /// SetBorder packet - sent after handshake.
    SetBorder {
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        scramble_x: i32,
        scramble_y: i32,
        game_type: u32,
        server_name: String,
    },
    /// ServerStat packet - JSON stats response.
    ServerStat { json: String },
    /// Chat message sent only to this client (server replies).
    ChatMessage {
        name: String,
        color: Color,
        message: String,
        is_server: bool,
    },
    /// XRay data packet (operator only).
    XrayData {
        player_cells: Vec<protocol::packets::XrayPlayerCell>,
        scramble_id: u32,
        scramble_x: i32,
        scramble_y: i32,
    },
}

/// Connection tracking state (shared across connection handlers).
struct ConnectionState {
    /// Number of connections per IP address.
    ip_connections: HashMap<IpAddr, usize>,
    /// Total number of connections.
    total_connections: usize,
    /// Banned IP addresses.
    ban_list: HashSet<IpAddr>,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            ip_connections: HashMap::new(),
            total_connections: 0,
            ban_list: HashSet::new(),
        }
    }

    /// Load ban list from file.
    fn load_ban_list(&mut self, path: &Path) {
        if !path.exists() {
            info!("No ban list file found at {:?}", path);
            return;
        }

        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let mut count = 0;
                for line in contents.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Ok(ip) = line.parse::<IpAddr>() {
                        self.ban_list.insert(ip);
                        count += 1;
                    } else {
                        warn!("Invalid IP in ban list: {}", line);
                    }
                }
                info!("Loaded {} IP bans from {:?}", count, path);
            }
            Err(e) => {
                warn!("Failed to load ban list from {:?}: {}", path, e);
            }
        }
    }

    /// Check if an IP is banned.
    fn is_banned(&self, ip: &IpAddr) -> bool {
        self.ban_list.contains(ip)
    }

    /// Try to add a connection, returns true if allowed.
    fn try_add_connection(&mut self, ip: IpAddr, max_total: usize, max_per_ip: usize) -> bool {
        // Check total connections
        if self.total_connections >= max_total {
            return false;
        }

        // Check per-IP limit
        let current = self.ip_connections.get(&ip).copied().unwrap_or(0);
        if current >= max_per_ip {
            return false;
        }

        // Add the connection
        *self.ip_connections.entry(ip).or_insert(0) += 1;
        self.total_connections += 1;
        true
    }

    /// Remove a connection.
    fn remove_connection(&mut self, ip: IpAddr) {
        if let Some(count) = self.ip_connections.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
                self.total_connections = self.total_connections.saturating_sub(1);
            }
            if *count == 0 {
                self.ip_connections.remove(&ip);
            }
        }
    }
}

/// Run the game server.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", config.server.bind, config.server.port).parse()?;
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on ws://{}", addr);

    // Connection tracking state
    let conn_state = Arc::new(RwLock::new(ConnectionState::new()));

    // Load ban list
    {
        let mut state = conn_state.write().await;
        state.load_ban_list(Path::new("banlist.txt"));
    }

    // Create broadcast channels for chat messages, leaderboard, world updates, and targeted messages
    let (chat_tx, _chat_rx) = broadcast::channel::<ChatBroadcast>(100);
    let (lb_tx, _lb_rx) = broadcast::channel::<LeaderboardBroadcast>(10);
    let (world_tx, _world_rx) = broadcast::channel::<WorldUpdateBroadcast>(5);
    let (targeted_tx, _targeted_rx) = broadcast::channel::<TargetedMessage>(100);

    // Shared game state
    let game_state = Arc::new(RwLock::new(GameState::new(&config, chat_tx.clone(), lb_tx.clone(), world_tx.clone(), targeted_tx.clone())));

    // Start the game loop
    let game_loop_state = Arc::clone(&game_state);
    let tick_interval = config.server.tick_interval_ms;
    tokio::spawn(async move {
        game::run_game_loop(game_loop_state, tick_interval).await;
    });

    // Connection limits
    let max_connections = config.server.max_connections;
    let ip_limit = config.server.ip_limit;

    loop {
        let (stream, addr) = listener.accept().await?;
        let ip = addr.ip();

        // Check ban list and connection limits
        {
            let mut state = conn_state.write().await;

            // Check if IP is banned
            if state.is_banned(&ip) {
                warn!("Connection rejected (IP banned): {}", addr);
                continue;
            }

            // Check connection limits
            if !state.try_add_connection(ip, max_connections, ip_limit) {
                warn!("Connection rejected (limit reached): {}", addr);
                continue;
            }
        }

        let game_state = Arc::clone(&game_state);
        let conn_state = Arc::clone(&conn_state);
        let chat_rx = chat_tx.subscribe();
        let lb_rx = lb_tx.subscribe();
        let world_rx = world_tx.subscribe();
        let targeted_rx = targeted_tx.subscribe();

        tokio::spawn(async move {
            let result = handle_connection(stream, addr, game_state, chat_rx, lb_rx, world_rx, targeted_rx).await;

            // Always remove from connection tracking when done
            {
                let mut state = conn_state.write().await;
                state.remove_connection(addr.ip());
            }

            if let Err(e) = result {
                error!("Connection error from {}: {}", addr, e);
            }
        });
    }
}

/// Handle a single WebSocket connection.
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    game_state: Arc<RwLock<GameState>>,
    mut chat_rx: broadcast::Receiver<ChatBroadcast>,
    mut lb_rx: broadcast::Receiver<LeaderboardBroadcast>,
    mut world_rx: broadcast::Receiver<WorldUpdateBroadcast>,
    mut targeted_rx: broadcast::Receiver<TargetedMessage>,
) -> anyhow::Result<()> {
    let ws_stream = accept_async(stream).await?;
    info!("New connection from {}", addr);

    let (mut write, mut read) = ws_stream.split();

    // Create client
    let client_id = {
        let mut state = game_state.write().await;
        state.add_client(addr)
    };

    // Note: ClearAll and SetBorder are sent after handshake completes (packet 255)

    // Track which nodes this client has seen (for delta updates)
    let mut client_nodes: HashSet<u32> = HashSet::new();

    // Message loop - handle both incoming messages and broadcasts
    loop {
        tokio::select! {
            // Handle incoming WebSocket messages
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let mut state = game_state.write().await;
                        if let Err(e) = state.handle_packet(client_id, &data) {
                            warn!("Packet error from {}: {}", addr, e);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
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
                        false, // is_admin
                        false, // is_mod
                    );
                    if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
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
                            // Teams mode (Pie chart)
                            let team_scores: Vec<f32> = lb.entries.iter()
                                .map(|e| e.score)
                                .collect();
                            let packet = protocol::packets::build_leaderboard_pie(&team_scores);
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send pie leaderboard to {}: {}", addr, e);
                                break;
                            }
                        }
                        _ => {
                            // FFA mode
                            let entries: Vec<(bool, &str)> = lb.entries.iter()
                                .take(10) // Top 10
                                .map(|e| (e.client_id == client_id, e.name.as_str()))
                                .collect();

                            let packet = protocol::packets::build_leaderboard_ffa(&entries);
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
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
                    // Get this client's view data
                    let client_view = match world.client_data.get(&client_id) {
                        Some(v) => v,
                        None => continue, // Client not in game yet
                    };

                    // Calculate viewport bounds
                    let scale = client_view.scale.max(0.15);
                    let view_half_w = (1920.0 / scale) / 2.0;
                    let view_half_h = (1080.0 / scale) / 2.0;
                    let view_min_x = client_view.center_x - view_half_w;
                    let view_min_y = client_view.center_y - view_half_h;
                    let view_max_x = client_view.center_x + view_half_w;
                    let view_max_y = client_view.center_y + view_half_h;

                    // Find cells in viewport
                    let mut view_nodes: HashSet<u32> = HashSet::new();
                    for cell in &world.cells {
                        // Check if cell is in viewport (with some margin for size)
                        let margin = cell.size;
                        if cell.x + margin >= view_min_x
                            && cell.x - margin <= view_max_x
                            && cell.y + margin >= view_min_y
                            && cell.y - margin <= view_max_y
                        {
                            view_nodes.insert(cell.node_id);
                        }
                    }

                    // Also always include own cells
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

                    // Calculate add/update/delete sets
                    let mut add_nodes = Vec::new();
                    let mut upd_nodes = Vec::new();
                    let mut del_nodes = Vec::new();

                    // Nodes to add (in view but not in client_nodes)
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
                                    is_spiked: cell.cell_type == 2, // Virus
                                    is_player: true, // Always send color (needed for Rainbow mode)
                                    has_skin: is_new && cell.skin.is_some(),
                                    has_name: is_new && cell.name.is_some(),
                                    is_agitated: false,
                                    is_ejected: cell.cell_type == 3,
                                    is_food: cell.cell_type == 1,
                                },
                                skin: if is_new { cell.skin.clone() } else { None },
                                name: if is_new { cell.name.clone() } else { None }, // Send name for all cells when adding
                            };

                            if is_new {
                                add_nodes.push(update_cell);
                            } else {
                                upd_nodes.push(update_cell);
                            }
                        }
                    }

                    // Nodes to delete (in client_nodes but not in view)
                    for &node_id in &client_nodes {
                        if !view_nodes.contains(&node_id) {
                            del_nodes.push(node_id);
                        }
                    }

                    // Build eat records
                    let eat_records: Vec<protocol::packets::EatRecord> = world.eaten.iter()
                        .filter(|(eaten_id, eater_id)| {
                            view_nodes.contains(eaten_id)
                                || view_nodes.contains(eater_id)
                                || client_nodes.contains(eaten_id)
                                || client_nodes.contains(eater_id)
                        })
                        .map(|&(eaten_id, eater_id)| protocol::packets::EatRecord { eaten_id, eater_id })
                        .collect();

                    // Update client_nodes
                    client_nodes = view_nodes;

                    // Build and send the packet
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

                    if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                        warn!("Failed to send world update to {}: {}", addr, e);
                        break;
                    }
                }
            }
            // Handle targeted messages (AddNode, etc.)
            targeted_msg = targeted_rx.recv() => {
                if let Ok(msg) = targeted_msg {
                    // Only process messages for this client
                    if msg.client_id != client_id {
                        continue;
                    }

                    match msg.message {
                        TargetedMessageType::AddNode { node_id, scramble_id } => {
                            let packet = protocol::packets::build_add_node(node_id, scramble_id);
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send AddNode to {}: {}", addr, e);
                                break;
                            }
                        }
                        TargetedMessageType::ClearAll => {
                            let packet = protocol::packets::build_clear_all();
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send ClearAll to {}: {}", addr, e);
                                break;
                            }
                        }
                        TargetedMessageType::SetBorder { min_x, min_y, max_x, max_y, scramble_x, scramble_y, game_type, server_name } => {
                            // Apply scramble to border coordinates (as the JS does)
                            let packet = protocol::packets::build_set_border(
                                min_x + scramble_x as f64,
                                min_y + scramble_y as f64,
                                max_x + scramble_x as f64,
                                max_y + scramble_y as f64,
                                game_type,
                                &server_name
                            );
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send SetBorder to {}: {}", addr, e);
                                break;
                            }
                        }
                        TargetedMessageType::ServerStat { json } => {
                            let packet = protocol::packets::build_server_stat(&json);
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send ServerStat to {}: {}", addr, e);
                                break;
                            }
                        }
                        TargetedMessageType::ChatMessage { name, color, message, is_server } => {
                            let packet = protocol::packets::build_chat_message(
                                color,
                                &name,
                                &message,
                                is_server,
                                false,
                                false,
                            );
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
                                warn!("Failed to send ChatMessage to {}: {}", addr, e);
                                break;
                            }
                        }
                        TargetedMessageType::XrayData { player_cells, scramble_id, scramble_x, scramble_y } => {
                            let packet = protocol::packets::build_xray_data(
                                scramble_id,
                                scramble_x,
                                scramble_y,
                                &player_cells,
                            );
                            if let Err(e) = write.send(Message::Binary(packet.finish().to_vec().into())).await {
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
