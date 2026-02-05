//! Game state and main loop.

use crate::ai::BotManager;
use crate::config::Config;
use crate::entity::{Cell, CellType, PlayerCell};
use crate::world::{CellEntry, World};
use protocol::packets::ClientPacket;
use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval_at, sleep, Instant, MissedTickBehavior};
use futures_util::FutureExt;
use tracing::{debug, info, warn};
use fixedbitset::FixedBitSet;

use super::client::Client;
use super::{ChatBroadcast, ClientViewData, LeaderboardBroadcast, TargetedMessage, TargetedMessageType, WorldCell, WorldUpdateBroadcast};

/// Pending broadcasts to send after releasing the game state lock.
pub struct PendingBroadcasts {
    pub world_update: Option<WorldUpdateBroadcast>,
    pub leaderboard: Option<LeaderboardBroadcast>,
    pub xray_messages: Vec<TargetedMessage>,
}

/// World border (for protocol compatibility).
#[derive(Debug, Clone)]
pub struct Border {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub width: f64,
    pub height: f64,
}

impl Border {
    pub fn new(width: f64, height: f64) -> Self {
        let half_w = width / 2.0;
        let half_h = height / 2.0;
        Self {
            min_x: -half_w,
            min_y: -half_h,
            max_x: half_w,
            max_y: half_h,
            width,
            height,
        }
    }
}

/// Main game state.
pub struct GameState {
    pub config: Config,
    pub border: Border,
    pub tick_count: u64,
    pub start_time: std::time::Instant,

    // ID counters
    next_client_id: u32,

    // Connected clients
    pub clients: HashMap<u32, Client>,

    // Game world (entities)
    pub world: World,

    // Bot manager
    pub bots: BotManager,

    // Chat broadcast channel
    chat_tx: broadcast::Sender<ChatBroadcast>,

    // Leaderboard broadcast channel
    lb_tx: broadcast::Sender<LeaderboardBroadcast>,

    // World update broadcast channel
    world_tx: broadcast::Sender<WorldUpdateBroadcast>,

    // Targeted message channel
    targeted_tx: broadcast::Sender<TargetedMessage>,

    // Tick count since last leaderboard update
    last_lb_tick: u64,

    // Track eaten cells this tick: (eaten_id, eater_id)
    eaten_this_tick: Vec<(u32, u32)>,
    // Track player deaths this tick: (killer_owner, victim_owner)
    deaths_this_tick: Vec<(u32, u32)>,

    // Average tick duration in milliseconds (exponential moving average).
    pub update_time_avg: f64,

    // Game mode
    pub gamemode: Box<dyn crate::gamemodes::GameMode>,

    // Reusable buffers for collision detection (reduce allocations)
    collision_owner_lookup: HashMap<u32, u32>,
    collision_remerge_lookup: HashMap<u32, bool>,
    collision_eat_events: Vec<(u32, u32, f32)>,
    collision_cells_to_remove: FixedBitSet,
    collision_virus_pops: Vec<(u32, u32)>,
    collision_virus_ate_eject: Vec<u32>,

    // Reusable buffers for movement and broadcast (reduce allocations)
    movement_cell_targets: Vec<(u32, f32, f32, u32)>,
    movement_speed_mults: HashMap<u32, f32>,
    broadcast_world_cells: Vec<WorldCell>,
    xray_client_ids: Vec<u32>,
}

impl GameState {
    /// Create a new game state.
    pub fn new(
        config: &Config,
        chat_tx: broadcast::Sender<ChatBroadcast>,
        lb_tx: broadcast::Sender<LeaderboardBroadcast>,
        world_tx: broadcast::Sender<WorldUpdateBroadcast>,
        targeted_tx: broadcast::Sender<TargetedMessage>,
    ) -> Self {
        let world = World::new(config.border.width as f32, config.border.height as f32);

        Self {
            config: config.clone(),
            border: Border::new(config.border.width, config.border.height),
            tick_count: 0,
            start_time: std::time::Instant::now(),
            next_client_id: 1,
            clients: HashMap::new(),
            world,
            bots: BotManager::new(),
            chat_tx,
            lb_tx,
            world_tx,
            targeted_tx,
            last_lb_tick: 0,
            eaten_this_tick: Vec::new(),
            deaths_this_tick: Vec::new(),
            update_time_avg: 0.0,
            gamemode: crate::gamemodes::get_gamemode(config.server.gamemode),
            // Pre-allocate reusable buffers based on typical game loads
            // Sized for 128 players with 16 cells each = ~2048 cells
            collision_owner_lookup: HashMap::with_capacity(2048),
            collision_remerge_lookup: HashMap::with_capacity(2048),
            collision_eat_events: Vec::with_capacity(256),  // More events per tick
            collision_cells_to_remove: FixedBitSet::with_capacity(10000),  // Large enough for typical cell IDs
            collision_virus_pops: Vec::with_capacity(32),
            collision_virus_ate_eject: Vec::with_capacity(64),
            // Movement and broadcast buffers
            movement_cell_targets: Vec::with_capacity(2048),
            movement_speed_mults: HashMap::with_capacity(128),
            broadcast_world_cells: Vec::with_capacity(5000),
            xray_client_ids: Vec::with_capacity(16),
        }
    }

    /// Add a new client.
    pub fn add_client(&mut self, addr: SocketAddr) -> u32 {
        let id = self.next_client_id;
        self.next_client_id += 1;
        let client = Client::new(id, addr);
        self.clients.insert(id, client);
        info!("Client {} connected from {}", id, addr);
        id
    }

    /// Remove a client.
    pub fn remove_client(&mut self, id: u32) {
        if let Some(client) = self.clients.remove(&id) {
            info!("Client {} ({}) disconnected", id, client.addr);
            // Remove all cells owned by this client
            let cell_ids: Vec<u32> = client.cells.clone();
            for cell_id in cell_ids {
                self.world.remove_cell(cell_id);
            }
            
            // Remove all minions owned by this client
            for minion_id in &client.minions {
                // First, remove all cells owned by the minion
                if let Some(bot) = self.bots.get_bot(*minion_id) {
                    let bot_cells: Vec<u32> = bot.cells.clone();
                    for cell_id in bot_cells {
                        self.world.remove_cell(cell_id);
                    }
                }
                // Then remove the minion bot itself
                self.bots.remove_bot(*minion_id);
            }
        }
    }

    /// Handle a packet from a client.
    pub fn handle_packet(&mut self, client_id: u32, data: &[u8]) -> anyhow::Result<()> {
        let client = self
            .clients
            .get_mut(&client_id)
            .ok_or_else(|| anyhow::anyhow!("Client not found"))?;

        client.touch();

        // Check handshake state
        if !client.handshake_complete {
            return self.handle_handshake(client_id, data);
        }

        // Parse packet
        let packet = ClientPacket::parse(data, client.protocol)?;
        if let ClientPacket::Mouse { .. } = packet {
            // Mouse packets are very frequent; avoid logging them
        } else if let ClientPacket::StatsRequest { .. } = packet {
            // StatsRequest packets are also frequent; avoid logging them
        } else {
            debug!("Client {} sent {:?}", client_id, packet);
        }
        match packet {
            ClientPacket::Join { name } => {
                self.handle_join(client_id, name)?;
            }
            ClientPacket::Spectate => {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.is_spectating = true;
                }
            }
            ClientPacket::Mouse { x, y } => {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.mouse_x = x - client.scramble_x;
                    client.mouse_y = y - client.scramble_y;
                }
            }
            ClientPacket::Split => {
                self.handle_split(client_id);
            }
            ClientPacket::Eject => {
                self.handle_eject(client_id);
            }
            ClientPacket::Chat { message, .. } => {
                self.handle_chat(client_id, message)?;
            }
            ClientPacket::StatsRequest => {
                self.handle_stats_request(client_id);
            }
            ClientPacket::KeyQ => {
                // Toggle player frozen (main cells stop, minions keep moving)
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.frozen = !client.frozen;
                    let state = if client.frozen { "frozen" } else { "unfrozen" };
                    self.send_server_message(client_id, &format!("You are {}.", state));
                }
            }
            ClientPacket::KeyE => {
                // Trigger one-shot minion split
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if client.minion_control && !client.minions.is_empty() {
                        client.minion_split = true;
                    }
                }
            }
            ClientPacket::KeyR => {
                // Trigger one-shot minion eject
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if client.minion_control && !client.minions.is_empty() {
                        client.minion_eject = true;
                    }
                }
            }
            ClientPacket::KeyT => {
                // Toggle minion frozen
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if client.minion_control && !client.minions.is_empty() {
                        client.minion_frozen = !client.minion_frozen;
                        let state = if client.minion_frozen { "true" } else { "false" };
                        self.send_server_message(client_id, &format!("Minions frozen: {}.", state));
                    }
                }
            }
            ClientPacket::KeyP => {
                // Toggle minion food collection
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if client.minion_control && !client.minions.is_empty() {
                        client.minion_collect = !client.minion_collect;
                        let state = if client.minion_collect { "on" } else { "off" };
                        self.send_server_message(client_id, &format!("Minion food collection: {}.", state));
                    }
                }
            }
            _ => {
                debug!("Unhandled packet: {:?}", packet);
            }
        }

        Ok(())
    }

    /// Handle handshake packets.
    fn handle_handshake(&mut self, client_id: u32, data: &[u8]) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let client = self
            .clients
            .get_mut(&client_id)
            .ok_or_else(|| anyhow::anyhow!("Client not found"))?;

        match data[0] {
            0xFE if data.len() == 5 => {
                // Protocol version
                let version = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
                if !(1..=17).contains(&version) {
                    warn!(
                        "Client {} sent unsupported protocol version {}",
                        client_id, version
                    );
                    return Err(anyhow::anyhow!("Unsupported protocol"));
                }
                client.protocol = version;
                debug!("Client {} using protocol {}", client_id, version);
            }
            0xFF if data.len() == 5 => {
                // Handshake key
                let key = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
                if client.protocol > 6 && key != 0 {
                    warn!("Client {} sent invalid handshake key", client_id);
                    return Err(anyhow::anyhow!("Invalid handshake key"));
                }
                client.handshake_complete = true;
                info!(
                    "Client {} handshake complete (protocol {})",
                    client_id, client.protocol
                );

                // Send ClearAll and SetBorder now that handshake is complete
                let _ = self.targeted_tx.send(TargetedMessage {
                    client_id,
                    message: TargetedMessageType::ClearAll,
                });

                let _ = self.targeted_tx.send(TargetedMessage {
                    client_id,
                    message: TargetedMessageType::SetBorder {
                        min_x: self.border.min_x,
                        min_y: self.border.min_y,
                        max_x: self.border.max_x,
                        max_y: self.border.max_y,
                        scramble_x: client.scramble_x,
                        scramble_y: client.scramble_y,
                        game_type: self.config.server.gamemode,
                        server_name: self.config.server.name.clone(),
                    },
                });
            }
            _ => {
                warn!("Client {} sent unexpected handshake packet", client_id);
            }
        }

        Ok(())
    }

    /// Handle join request.
    fn handle_join(&mut self, client_id: u32, name: String) -> anyhow::Result<()> {
        // Parse name and skin
        let (skin, player_name) = parse_name_and_skin(&name);
        let player_name: String = player_name
            .chars()
            .take(self.config.player.max_nick_length)
            .collect();

        // Update client
        {
            let client = self
                .clients
                .get_mut(&client_id)
                .ok_or_else(|| anyhow::anyhow!("Client not found"))?;
            client.name = player_name.clone();
            client.skin = skin;
            
            // Let GameMode handle team assignment etc.
            self.gamemode.on_player_join(client);
        }

        let team = self.clients.get(&client_id).and_then(|c| c.team);

        info!(
            "Client {} joined as '{}'{}",
            client_id,
            if player_name.is_empty() {
                "An unnamed cell"
            } else {
                &player_name
            },
            if let Some(t) = team {
                format!(" (Team {})", t)
            } else {
                "".to_string()
            }
        );

        // Spawn player cell only if they don't already have any
        let has_cells = self.world.cells.values()
            .filter_map(|cell| {
                if let CellEntry::Player(player_cell) = cell {
                    player_cell.cell_data.owner_id
                } else {
                    None
                }
            })
            .any(|owner| owner == client_id);
        if !has_cells {
            self.spawn_player(client_id);
        }

        // Spawn default minions if configured
        let minion_count = self.config.server.server_minions;
        if minion_count > 0 {
            self.spawn_default_minions(client_id, minion_count);
        }

        Ok(())
    }

    /// Get fuzzy color for a team (matching Teams.js fuzzColor)
    #[allow(dead_code)]
    fn get_team_color(team: u8) -> protocol::Color {
        let mut rng = rand::rng();
        let fuzz = 38;
        
        let base_color = match team {
            0 => (255, 0, 0), // Red
            1 => (0, 255, 0), // Green
            _ => (0, 0, 255), // Blue
        };

        let r = (base_color.0 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;
        let g = (base_color.1 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;
        let b = (base_color.2 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;

        protocol::Color::new(r, g, b)
    }

    /// Spawn a player cell for the given client.
    pub fn spawn_player(&mut self, client_id: u32) {
        let start_size = self.config.player.start_size as f32;
        let position = self.world.border.random_position();
        let node_id = self.world.next_id();

        let mut cell = PlayerCell::new(node_id, client_id, position, start_size, self.tick_count);

        // Get client color and scramble_id
        let scramble_id = if let Some(client) = self.clients.get_mut(&client_id) {
            // Let GameMode specialize the client if needed (e.g. refresh fuzzy color)
            self.gamemode.on_player_spawn(client);
            
            cell.cell_data.color = client.color;
            client.scramble_id
        } else {
            0
        };

        let cell_id = self.world.add_player_cell(cell);

        // Add to client's cell list
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.cells.push(cell_id);
        }

        // Send AddNode packet to tell client which cell is theirs
        let _ = self.targeted_tx.send(TargetedMessage {
            client_id,
            message: TargetedMessageType::AddNode {
                node_id: cell_id,
                scramble_id,
            },
        });

        info!("Spawned player cell {} for client {}", cell_id, client_id);
    }

    /// Handle split request (Space key).
    fn handle_split(&mut self, client_id: u32) {
        let max_cells = self.config.player.max_cells;
        let min_split_size = self.config.player.min_split_size as f32;
        let split_speed = self.config.player.split_speed as f32;

        // Get client or bot data
        let (cell_ids, mouse_x, mouse_y, scramble_id): (Vec<u32>, f32, f32, u32) = if let Some(client) = self.clients.get(&client_id) {
            (client.cells.clone(), client.mouse_x as f32, client.mouse_y as f32, client.scramble_id)
        } else if let Some(bot) = self.bots.get_bot(client_id) {
            (bot.cells.clone(), bot.target.x, bot.target.y, 0)
        } else {
            return;
        };

        if cell_ids.is_empty() || cell_ids.len() >= max_cells {
            return;
        }

        // Collect cells that can split
        let mut cells_to_split = Vec::new();
        for &cell_id in &cell_ids {
            if cell_ids.len() + cells_to_split.len() >= max_cells {
                break;
            }
            if let Some(cell) = self.world.get_cell(cell_id) {
                if cell.data().size > min_split_size {
                    cells_to_split.push(cell_id);
                }
            }
        }

        if cells_to_split.is_empty() {
            return;
        }

        debug!("Client/Bot {} splitting {} cells", client_id, cells_to_split.len());

        // Process each cell split
        for cell_id in cells_to_split {
            // Check if still under max cells
            if let Some(client) = self.clients.get(&client_id) {
                if client.cells.len() >= max_cells {
                    break;
                }
            }

            // Get cell data
            let (position, size, color) = match self.world.get_cell(cell_id) {
                Some(cell) => {
                    let data = cell.data();
                    (data.position, data.size, data.color)
                }
                None => continue,
            };

            // Calculate split angle toward mouse
            let dx = mouse_x as f32 - position.x;
            let dy = mouse_y as f32 - position.y;
            let angle = if dx * dx + dy * dy < 1.0 {
                0.0 // No direction, split straight up
            } else {
                dy.atan2(dx)
            };

            // Calculate new size (split in half)
            // JS: parent._size / Math.sqrt(2) = parent._size / 1.414
            let new_size = size / 1.414213; // sqrt(2)

            if new_size < self.config.player.min_size as f32 {
                continue;
            }

            // Shrink parent cell
            if let Some(cell) = self.world.get_cell_mut(cell_id) {
                cell.data_mut().set_size(new_size);
            }
            self.world.update_cell_position(cell_id);

            // Create new split cell
            let new_id = self.world.next_id();
            let mut new_cell = crate::entity::PlayerCell::new(
                new_id,
                client_id,
                position, // Start at same position
                new_size,
                self.tick_count,
            );
            new_cell.cell_data.color = color;

            // Apply boost in split direction
            // JS: cell.setBoost(this.config.playerSplitSpeed * Math.pow(size, .0122), angle)
            let boost_distance = split_speed * new_size.powf(0.0122);
            let boost_dir = glam::Vec2::new(angle.cos(), angle.sin());
            new_cell.cell_data.set_boost_direction(boost_distance, boost_dir);

            // Add new cell to world
            let cell_id = self.world.add_player_cell(new_cell);

            // Add to moving cells
            self.world.add_moving(cell_id);

            // Add to client's or bot's cell list
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.cells.push(cell_id);
            } else if let Some(bot) = self.bots.get_bot_mut(client_id) {
                bot.cells.push(cell_id);
            }

            // Send AddNode packet to tell client which cell is theirs
            let _ = self.targeted_tx.send(TargetedMessage {
                client_id,
                message: TargetedMessageType::AddNode {
                    node_id: cell_id,
                    scramble_id,
                },
            });
        }
    }

    /// Split a player cell into a new cell with a specific mass (used for virus popping).
    fn split_player_cell_with_mass(&mut self, owner_id: u32, parent_cell_id: u32, angle: f32, new_cell_mass: f32) {
        let max_cells = self.config.player.max_cells;
        let split_speed = self.config.player.split_speed as f32;

        // Check cell count
        let current_count = if let Some(client) = self.clients.get(&owner_id) {
            if client.cells.len() >= max_cells {
                return;
            }
            client.cells.len()
        } else if let Some(bot) = self.bots.get_bot(owner_id) {
            if bot.cells.len() >= max_cells {
                return;
            }
            bot.cells.len()
        } else {
            return;
        };

        if current_count >= max_cells {
            return;
        }

        // Get parent cell data
        let (position, color, parent_size) = match self.world.get_cell(parent_cell_id) {
            Some(cell) => {
                let data = cell.data();
                (data.position, data.color, data.size)
            }
            None => return,
        };

        let scramble_id = if let Some(client) = self.clients.get(&owner_id) {
            client.scramble_id
        } else {
            0
        };

        // Calculate new size from mass: size = sqrt(mass * 100)
        let new_size = (new_cell_mass * 100.0).sqrt();

        if new_size < self.config.player.min_size as f32 {
            return;
        }

        // Shrink parent (JS: size2 = sqrt(parent.radius - size1²); parent.setSize(size2))
        // radius = size², so new_parent_size = sqrt(parent_size² - new_size²)
        let new_parent_size_sq = parent_size * parent_size - new_size * new_size;
        if new_parent_size_sq <= 0.0 {
            return;
        }
        let new_parent_size = new_parent_size_sq.sqrt();
        if new_parent_size < self.config.player.min_size as f32 {
            return; // JS: if (isNaN(size2) || size2 < playerMinDecay) return
        }
        if let Some(parent) = self.world.get_cell_mut(parent_cell_id) {
            parent.data_mut().set_size(new_parent_size);
        }
        self.world.update_cell_position(parent_cell_id);

        // Create new split cell
        let new_id = self.world.next_id();
        let mut new_cell = crate::entity::PlayerCell::new(
            new_id,
            owner_id,
            position,
            new_size,
            self.tick_count,
        );
        new_cell.cell_data.color = color;

        // Apply boost in split direction
        let boost_distance = split_speed * new_size.powf(0.0122);
        let boost_dir = glam::Vec2::new(angle.cos(), angle.sin());
        new_cell.cell_data.set_boost_direction(boost_distance, boost_dir);

        // Add new cell to world
        let cell_id = self.world.add_player_cell(new_cell);

        // Add to moving cells
        self.world.add_moving(cell_id);

        // Add to owner's cell list
        if let Some(client) = self.clients.get_mut(&owner_id) {
            client.cells.push(cell_id);
        } else if let Some(bot) = self.bots.get_bot_mut(owner_id) {
            bot.cells.push(cell_id);
        }

        // Send AddNode packet to tell client which cell is theirs
        let _ = self.targeted_tx.send(TargetedMessage {
            client_id: owner_id,
            message: TargetedMessageType::AddNode {
                node_id: cell_id,
                scramble_id,
            },
        });
    }

    /// Handle eject request (W key).
    fn handle_eject(&mut self, client_id: u32) {
        let eject_cooldown = self.config.eject.cooldown as u64;
        let min_eject_size = self.config.player.min_eject_size as f32;
        let eject_size_loss = self.config.eject.size_loss as f32;
        let eject_size = self.config.eject.size as f32;
        let eject_speed = self.config.eject.speed as f32;
        let tick_count = self.tick_count;

        // Get cells and mouse/target position, with cooldown check for human clients
        let (cell_ids, mouse_x, mouse_y): (Vec<u32>, i32, i32) = if let Some(client) = self.clients.get_mut(&client_id) {
            if client.cells.is_empty() {
                return;
            }
            if tick_count.saturating_sub(client.last_eject_tick) < eject_cooldown {
                return;
            }
            client.last_eject_tick = tick_count;
            (client.cells.clone(), client.mouse_x, client.mouse_y)
        } else if let Some(bot) = self.bots.get_bot(client_id) {
            if bot.cells.is_empty() {
                return;
            }
            (bot.cells.clone(), bot.target.x as i32, bot.target.y as i32)
        } else {
            return;
        };

        debug!("Client {} ejecting from {} cells", client_id, cell_ids.len());

        // Process each cell
        for cell_id in cell_ids {
            // Get cell data
            let (cell_pos, cell_size, cell_color) = match self.world.get_cell(cell_id) {
                Some(cell) => {
                    let data = cell.data();
                    (data.position, data.size, data.color)
                }
                None => continue,
            };

            // Check if cell is big enough to eject
            if cell_size < min_eject_size {
                continue;
            }

            // Calculate direction toward mouse
            let dx = mouse_x as f32 - cell_pos.x;
            let dy = mouse_y as f32 - cell_pos.y;
            let squared = dx * dx + dy * dy;
            let (norm_dx, norm_dy) = if squared > 1.0 {
                let dist = squared.sqrt();
                (dx / dist, dy / dist)
            } else {
                (0.0, 0.0)
            };

            // Shrink the cell
            // JS: cell.setSize(Math.sqrt(cell.radius - loss * loss))
            let cell_radius = cell_size * cell_size;
            let new_radius = cell_radius - eject_size_loss * eject_size_loss;
            if new_radius <= 0.0 {
                continue;
            }
            let new_size = new_radius.sqrt();

            if let Some(cell) = self.world.get_cell_mut(cell_id) {
                cell.data_mut().set_size(new_size);
            }
            self.world.update_cell_position(cell_id);

            // Spawn position: at the edge of the cell in the eject direction
            let spawn_pos = glam::Vec2::new(
                cell_pos.x + norm_dx * new_size,
                cell_pos.y + norm_dy * new_size,
            );

            // Calculate eject angle
            let angle = if norm_dx == 0.0 && norm_dy == 0.0 {
                std::f32::consts::FRAC_PI_2
            } else {
                // Add some random variation
                let mut rng = rand::rng();
                let base_angle = norm_dx.atan2(norm_dy);
                base_angle + rng.random_range(-0.3..0.3)
            };

            // Create ejected mass
            let eject_id = self.world.next_id();
            let mut eject = crate::entity::EjectedMass::new(eject_id, spawn_pos, eject_size, tick_count);
            eject.set_color(cell_color);
            eject.data_mut().set_boost(eject_speed, angle);

            // Add to world
            let new_id = self.world.add_eject(eject);
            self.world.add_moving(new_id);
        }
    }

    /// Handle chat message.
    fn handle_chat(&mut self, client_id: u32, message: String) -> anyhow::Result<()> {
        let client = self
            .clients
            .get(&client_id)
            .ok_or_else(|| anyhow::anyhow!("Client not found"))?;

        let name = if client.name.is_empty() {
            "An unnamed cell".to_string()
        } else {
            client.name.clone()
        };

        let color = client.color;

        // Check for commands
        if message.starts_with('/') {
            self.handle_command(client_id, &message)?;
            return Ok(());
        }

        info!("[Chat] {}: {}", name, message);

        // Broadcast to all clients
        let _ = self.chat_tx.send(ChatBroadcast {
            name,
            color,
            message,
            is_server: false,
        });

        Ok(())
    }

    /// Handle a StatsRequest packet — rate-limited to once per 30 ticks (matches JS).
    fn handle_stats_request(&mut self, client_id: u32) {
        let client = match self.clients.get_mut(&client_id) {
            Some(c) => c,
            None => return,
        };

        // Rate-limit: at most once every 30 ticks
        if self.tick_count.saturating_sub(client.last_stat_tick) < 30 {
            return;
        }
        client.last_stat_tick = self.tick_count;

        // Count player states
        let mut players_alive = 0u32;
        let mut players_dead = 0u32;
        let mut players_spect = 0u32;
        for c in self.clients.values() {
            if c.is_spectating {
                players_spect += 1;
            } else if c.cells.is_empty() {
                players_dead += 1;
            } else {
                players_alive += 1;
            }
        }
        let players_total = players_alive + players_dead + players_spect;
        let bots_total = self.bots.bots.len() as u32;

        let uptime_secs = self.start_time.elapsed().as_secs();
        let update_str = format!("{:.2}", self.update_time_avg);

        // Build JSON matching JS ServerStat output
        let json = format!(
            r#"{{"name":"{}","mode":"{}","uptime":{},"update":"{}","playersTotal":{},"playersAlive":{},"playersDead":{},"playersSpect":{},"botsTotal":{},"playersLimit":{}}}"#,
            self.config.server.name,
            self.gamemode.name(),
            uptime_secs,
            update_str,
            players_total,
            players_alive,
            players_dead,
            players_spect,
            bots_total,
            self.config.server.max_connections,
        );

        let _ = self.targeted_tx.send(TargetedMessage {
            client_id,
            message: TargetedMessageType::ServerStat { json },
        });
    }

    /// Handle a chat command.
    fn handle_command(&mut self, client_id: u32, command: &str) -> anyhow::Result<()> {
        let parts: Vec<&str> = command[1..].splitn(2, ' ').collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        let args = parts.get(1).copied().unwrap_or("");

        let is_op = self.clients.get(&client_id).map_or(false, |c| c.is_operator);

        match cmd.as_str() {
            // --- Public commands (no OP required) ---
            "help" => {
                if is_op {
                    self.send_server_message(client_id, "Operator commands: /operator, /list, /addbot, /kick, /kill, /killall, /mass, /speed, /freeze, /teleport, /gamemode, /chat, /name, /xray, /status");
                } else {
                    self.send_server_message(client_id, "Available commands: /help, /name, /operator <password>");
                }
            }
            "name" => {
                if let Some(client) = self.clients.get(&client_id) {
                    self.send_server_message(
                        client_id,
                        &format!(
                            "Your name is: {}",
                            if client.name.is_empty() { "An unnamed cell" } else { &client.name }
                        ),
                    );
                }
            }
            "operator" | "op" => {
                self.handle_cmd_operator(client_id, args);
            }
            // --- Operator commands ---
            "list" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                let mut msg = String::from("Players:");
                for (id, c) in &self.clients {
                    let name = if c.name.is_empty() { "unnamed" } else { &c.name };
                    msg.push_str(&format!(" [{}]{}", id, name));
                }
                msg.push_str(&format!(" | Bots: {}", self.bots.bots.len()));
                self.send_server_message(client_id, &msg);
            }
            "addbot" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                let count: usize = args.parse().unwrap_or(1);
                for _ in 0..count.min(10) {
                    self.bots.add_bot();
                }
                self.send_server_message(client_id, &format!("Added {} bot(s)", count.min(10)));
            }
            "kick" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                // Kick by ID
                if let Ok(target_id) = args.trim().parse::<u32>() {
                    if self.clients.contains_key(&target_id) {
                        self.remove_client(target_id);
                        self.send_server_message(client_id, &format!("Kicked client {}", target_id));
                    } else {
                        self.send_server_message(client_id, "Client not found.");
                    }
                } else {
                    self.send_server_message(client_id, "Usage: /kick <client_id>");
                }
            }
            "kill" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                self.handle_cmd_kill(client_id, args);
            }
            "killall" | "ka" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                // Kill all players except self
                let ids: Vec<u32> = self.clients.keys().filter(|&&id| id != client_id).copied().collect();
                for target_id in ids {
                    let cell_ids: Vec<u32> = self.clients.get(&target_id)
                        .map(|c| c.cells.clone()).unwrap_or_default();
                    for cell_id in cell_ids {
                        self.world.remove_cell(cell_id);
                    }
                    if let Some(c) = self.clients.get_mut(&target_id) {
                        c.cells.clear();
                    }
                }
                self.send_server_message(client_id, "All other players killed.");
            }
            "mass" | "m" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                self.handle_cmd_mass(client_id, args);
            }
            "speed" | "s" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                if let Ok(val) = args.trim().parse::<f64>() {
                    self.config.player.speed = val;
                    self.send_server_message(client_id, &format!("Speed set to {}", val));
                } else {
                    self.send_server_message(client_id, &format!("Current speed: {}. Usage: /speed <value>", self.config.player.speed));
                }
            }
            "freeze" | "f" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                // Freeze = set speed to 0, toggle
                if self.config.player.speed == 0.0 {
                    self.config.player.speed = 30.0;
                    self.send_server_message(client_id, "Unfrozen.");
                } else {
                    self.config.player.speed = 0.0;
                    self.send_server_message(client_id, "Frozen.");
                }
            }
            "teleport" | "tp" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                self.handle_cmd_teleport(client_id, args);
            }
            "gamemode" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                if let Ok(mode_id) = args.trim().parse::<u32>() {
                    self.gamemode = crate::gamemodes::get_gamemode(mode_id);
                    self.config.server.gamemode = mode_id;
                    self.send_server_message(client_id, &format!("Game mode changed to: {}", self.gamemode.name()));
                } else {
                    self.send_server_message(client_id, &format!("Current mode: {} ({}). Usage: /gamemode <id>", self.gamemode.name(), self.gamemode.id()));
                }
            }
            "chat" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                // Broadcast a server chat message
                if !args.is_empty() {
                    let _ = self.chat_tx.send(ChatBroadcast {
                        name: "SERVER".to_string(),
                        color: protocol::Color::new(255, 0, 0),
                        message: args.to_string(),
                        is_server: true,
                    });
                }
            }
            "minion" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                self.handle_cmd_minion(client_id, args);
            }
            "xray" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                self.handle_cmd_xray(client_id);
            }
            "status" => {
                if !is_op { self.send_server_message(client_id, "Operator only."); return Ok(()); }
                let uptime = self.start_time.elapsed().as_secs();
                let players = self.clients.len();
                let bots = self.bots.bots.len();
                let cells = self.world.cell_counts();
                self.send_server_message(client_id, &format!(
                    "Uptime: {}s | Players: {} | Bots: {} | Food: {} | Viruses: {} | Speed: {}",
                    uptime, players, bots, cells.food, cells.viruses, self.config.player.speed
                ));
            }
            _ => {
                self.send_server_message(client_id, &format!("Unknown command: /{}. Type /help for help.", cmd));
            }
        }

        Ok(())
    }

    /// Handle /operator command.
    fn handle_cmd_operator(&mut self, client_id: u32, args: &str) {
        let password = &self.config.server.operator_password;
        if password.is_empty() {
            self.send_server_message(client_id, "Operator mode is not configured.");
            return;
        }

        let client = match self.clients.get_mut(&client_id) {
            Some(c) => c,
            None => return,
        };

        if client.is_operator {
            // Toggle off
            client.is_operator = false;
            self.send_server_message(client_id, "Operator mode disabled.");
        } else if args.trim() == *password {
            client.is_operator = true;
            self.send_server_message(client_id, "Operator mode enabled.");
        } else {
            self.send_server_message(client_id, "Invalid password.");
        }
    }

    /// Handle /kill command.
    fn handle_cmd_kill(&mut self, client_id: u32, args: &str) {
        let target_id: u32 = match args.trim().parse() {
            Ok(id) => id,
            Err(_) => {
                self.send_server_message(client_id, "Usage: /kill <client_id>");
                return;
            }
        };

        let cell_ids: Vec<u32> = self.clients.get(&target_id)
            .map(|c| c.cells.clone())
            .unwrap_or_default();

        if cell_ids.is_empty() {
            self.send_server_message(client_id, "Target has no cells.");
            return;
        }

        for cell_id in cell_ids {
            self.world.remove_cell(cell_id);
        }
        if let Some(c) = self.clients.get_mut(&target_id) {
            c.cells.clear();
        }
        self.send_server_message(client_id, &format!("Killed client {}", target_id));
    }

    /// Handle /mass command — set all cells of self (or target) to a given size.
    fn handle_cmd_mass(&mut self, client_id: u32, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let (target_id, mass) = match parts.len() {
            1 => {
                // /mass <value> — applies to self
                match parts[0].parse::<f32>() {
                    Ok(m) => (client_id, m),
                    Err(_) => {
                        self.send_server_message(client_id, "Usage: /mass <value> or /mass <id> <value>");
                        return;
                    }
                }
            }
            2 => {
                // /mass <id> <value>
                match (parts[0].parse::<u32>(), parts[1].parse::<f32>()) {
                    (Ok(id), Ok(m)) => (id, m),
                    _ => {
                        self.send_server_message(client_id, "Usage: /mass <id> <value>");
                        return;
                    }
                }
            }
            _ => {
                self.send_server_message(client_id, "Usage: /mass <value> or /mass <id> <value>");
                return;
            }
        };

        // size = sqrt(mass) since mass = size^2 / 100 ... actually in JS mass = radius/100 = size^2/100
        let new_size = (mass * 100.0).sqrt();

        let cell_ids: Vec<u32> = self.clients.get(&target_id)
            .map(|c| c.cells.clone())
            .unwrap_or_default();

        if cell_ids.is_empty() {
            self.send_server_message(client_id, "Target has no cells.");
            return;
        }

        for cell_id in &cell_ids {
            if let Some(cell) = self.world.get_cell_mut(*cell_id) {
                cell.data_mut().set_size(new_size);
            }
            self.world.update_cell_position(*cell_id);
        }
        self.send_server_message(client_id, &format!("Set {} cells to mass {}", cell_ids.len(), mass));
    }

    /// Handle /teleport command — move self (or target) to given coordinates.
    fn handle_cmd_teleport(&mut self, client_id: u32, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let (target_id, x, y) = match parts.len() {
            2 => {
                match (parts[0].parse::<f32>(), parts[1].parse::<f32>()) {
                    (Ok(x), Ok(y)) => (client_id, x, y),
                    _ => {
                        self.send_server_message(client_id, "Usage: /teleport <x> <y> or /teleport <id> <x> <y>");
                        return;
                    }
                }
            }
            3 => {
                match (parts[0].parse::<u32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>()) {
                    (Ok(id), Ok(x), Ok(y)) => (id, x, y),
                    _ => {
                        self.send_server_message(client_id, "Usage: /teleport <id> <x> <y>");
                        return;
                    }
                }
            }
            _ => {
                self.send_server_message(client_id, "Usage: /teleport <x> <y> or /teleport <id> <x> <y>");
                return;
            }
        };

        let cell_ids: Vec<u32> = self.clients.get(&target_id)
            .map(|c| c.cells.clone())
            .unwrap_or_default();

        if cell_ids.is_empty() {
            self.send_server_message(client_id, "Target has no cells.");
            return;
        }

        for cell_id in &cell_ids {
            if let Some(cell) = self.world.get_cell_mut(*cell_id) {
                cell.data_mut().position = glam::Vec2::new(x, y);
            }
            self.world.update_cell_position(*cell_id);
        }
        self.send_server_message(client_id, &format!("Teleported client {} to ({}, {})", target_id, x, y));
    }

    /// Handle /minion command — add or remove minions for the operator.
    fn handle_cmd_minion(&mut self, client_id: u32, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let action = parts.first().copied().unwrap_or("");

        if action == "remove" || (action.is_empty() && self.clients.get(&client_id).map_or(false, |c| c.minion_control)) {
            // Remove all minions
            let minion_ids: Vec<u32> = self.clients.get(&client_id)
                .map(|c| c.minions.clone())
                .unwrap_or_default();
            for mid in &minion_ids {
                // Remove minion cells and bot entry
                if let Some(bot) = self.bots.get_bot(*mid) {
                    let cells: Vec<u32> = bot.cells.clone();
                    for cell_id in cells {
                        self.world.remove_cell(cell_id);
                    }
                }
                self.bots.remove_bot(*mid);
            }
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.minions.clear();
                client.latest_minion_id = 0;
                client.minion_control = false;
                client.minion_follow = false;
                client.minion_frozen = false;
                client.minion_collect = false;
            }
            self.send_server_message(client_id, "Successfully removed your minions.");
        } else {
            // Add minions
            let count: usize = action.parse().unwrap_or(1);
            let count = count.min(10); // Cap at 10

            let (owner_color, owner_name) = if let Some(client) = self.clients.get(&client_id) {
                let name = if client.name.is_empty() {
                    "Player".to_string()
                } else {
                    client.name.clone()
                };
                (client.color, name)
            } else {
                (protocol::Color::new(128, 128, 128), "Player".to_string())
            };
            
            let mut added = 0;
            for _ in 0..count {
                let minion_id = self.bots.add_bot();
                
                // Increment client's minion counter and get number
                let minion_number = if let Some(client) = self.clients.get_mut(&client_id) {
                    client.latest_minion_id += 1;
                    client.latest_minion_id
                } else {
                    1
                };
                
                // Set minion bot to use owner's color
                if let Some(bot) = self.bots.get_bot_mut(minion_id) {
                    bot.color = owner_color;
                    bot.name = format!("{} {}", owner_name, minion_number);
                    bot.needs_respawn = true;
                }
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.minions.push(minion_id);
                    client.minion_control = true;
                }
                added += 1;
            }
            self.send_server_message(client_id, &format!("You gave yourself {} minion(s). Use Q/E/R/T/P keys to control them.", added));
        }
    }

    /// Spawn default minions for a player (called on join if server_minions > 0).
    fn spawn_default_minions(&mut self, client_id: u32, count: usize) {
        let count = count.min(10); // Cap at 10

        // Get owner color and name
        let (owner_color, owner_name) = if let Some(client) = self.clients.get(&client_id) {
            let color = if self.config.player.minion_same_color {
                client.color
            } else {
                protocol::Color::new(0, 0, 0) // Will be set per minion below
            };
            let name = if client.name.is_empty() {
                "Player".to_string()
            } else {
                client.name.clone()
            };
            (color, name)
        } else {
            (protocol::Color::new(128, 128, 128), "Player".to_string())
        };

        for _ in 0..count {
            let minion_id = self.bots.add_bot();
            
            // Increment client's minion counter and get number
            let minion_number = if let Some(client) = self.clients.get_mut(&client_id) {
                client.latest_minion_id += 1;
                client.latest_minion_id
            } else {
                1
            };
            
            // Configure minion bot
            if let Some(bot) = self.bots.get_bot_mut(minion_id) {
                if self.config.player.minion_same_color {
                    bot.color = owner_color;
                } else {
                    bot.color = crate::world::World::random_color();
                }
                bot.name = format!("{} {}", owner_name, minion_number);
                bot.needs_respawn = true;
            }
            
            // Add to client's minion list
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.minions.push(minion_id);
                client.minion_control = true;
            }
        }

        info!("Client {} spawned with {} default minions", client_id, count);
    }

    /// Handle /xray command — toggle XRay mode to see all players.
    fn handle_cmd_xray(&mut self, client_id: u32) {
        let (status, info, client_name) = {
            let client = match self.clients.get_mut(&client_id) {
                Some(c) => c,
                None => return,
            };

            client.xray_enabled = !client.xray_enabled;
            let status = if client.xray_enabled { "enabled" } else { "disabled" };
            let info = if client.xray_enabled {
                "All players are now visible on your minimap."
            } else {
                "Normal visibility restored."
            };
            (status, info, client.name.clone())
        };
        
        self.send_server_message(client_id, &format!("Xray mode {}. {}", status, info));
        info!("{} {} xray mode.", client_name, status);
    }

    /// Send a server message to a specific client via targeted channel.
    fn send_server_message(&self, client_id: u32, message: &str) {
        let _ = self.targeted_tx.send(TargetedMessage {
            client_id,
            message: TargetedMessageType::ChatMessage {
                name: "SERVER".to_string(),
                color: protocol::Color::new(255, 0, 0),
                message: message.to_string(),
                is_server: true,
            },
        });
    }

    /// Run a single game tick and return pending broadcasts.
    pub fn tick(&mut self) -> PendingBroadcasts {
        let tick_start = std::time::Instant::now();
        
        self.tick_count += 1;
        self.eaten_this_tick.clear();
        self.deaths_this_tick.clear();

        // Spawn food if needed
        let spawn_start = std::time::Instant::now();
        self.world.spawn_food(
            self.config.food.min_amount,
            self.config.food.max_amount,
            self.config.food.spawn_amount,
            self.config.food.min_size as f32,
            self.config.food.max_size as f32,
            self.tick_count,
        );

        // Spawn viruses if needed
        self.world.spawn_viruses(
            self.config.virus.min_amount,
            self.config.virus.max_amount,
            self.config.virus.min_size as f32,
            self.tick_count,
        );
        let spawn_time = spawn_start.elapsed();

        // Update bots AI
        let ai_start = std::time::Instant::now();
        let mut team_lookup = HashMap::new();
        for client in self.clients.values() {
            if let Some(t) = client.team {
                team_lookup.insert(client.id, t);
            }
        }
        for bot in &self.bots.bots {
            if let Some(t) = bot.team {
                team_lookup.insert(bot.id, t);
            }
        }
        // Collect all minion IDs so the bot AI skips them — minions are
        // controlled exclusively via process_minions(), not independent AI.
        let minion_ids: std::collections::HashSet<u32> = self.clients.values()
            .flat_map(|c| c.minions.iter().copied())
            .collect();

        self.bots.update(&mut self.world, &self.config, &team_lookup, &minion_ids);

        // Handle bot split requests (minions excluded — they only split on
        // explicit owner command via process_minions)
        let bot_splits: Vec<u32> = self.bots.bots.iter()
            .filter(|b| b.split_requested && !minion_ids.contains(&b.id))
            .map(|b| b.id)
            .collect();
        for bot_id in bot_splits {
            self.handle_split(bot_id);
        }

        // Handle bot respawns
        self.process_bot_respawns();

        // Process minion control flags
        self.process_minions();
        let ai_time = ai_start.elapsed();

        // Update moving cells (boost physics)
        let movement_start = std::time::Instant::now();
        self.update_moving_cells();

        // Update player cell movement (including bots)
        self.update_player_movement();

        // Update bot movement toward their targets
        self.update_bot_movement();

        // Update merge status for all player cells BEFORE collision detection
        // This ensures cells can merge immediately when they become eligible
        self.update_merge_status();
        let movement_time = movement_start.elapsed();

        // Collision detection and eating
        let collision_start = std::time::Instant::now();
        self.process_collisions();

        // Detect deaths and notify gamemode (for Beatdown kill tracking, etc.)
        self.process_deaths();

        // Game mode tick logic (MotherCell spawning, Rainbow colors, etc.)
        // We need to temporarily take ownership to satisfy borrow checker
        let mut gamemode = std::mem::replace(&mut self.gamemode, Box::new(crate::gamemodes::ffa::Ffa::new()));
        gamemode.on_tick(self);
        self.gamemode = gamemode;
        let collision_time = collision_start.elapsed();

        // Cell decay (every 25 ticks)
        let decay_start = std::time::Instant::now();
        if self.tick_count % 25 == 0 {
            self.update_decay();
        }
        let decay_time = decay_start.elapsed();

        // Prepare leaderboard broadcast (every 25 ticks)
        let leaderboard_broadcast = if self.tick_count - self.last_lb_tick >= 25 {
            self.last_lb_tick = self.tick_count;
            Some(self.prepare_leaderboard_broadcast())
        } else {
            None
        };

        let total_time = tick_start.elapsed();

        // Prepare world state broadcast
        let broadcast_start = std::time::Instant::now();
        let (world_broadcast, xray_messages) = self.prepare_world_broadcast();
        let broadcast_time = broadcast_start.elapsed();

        // Log performance metrics every 400 ticks
        if self.tick_count % 400 == 0 {
            let entity_count = self.world.cells.len();
            let player_count = self.clients.len() + self.bots.bots.len();
            debug!(
                "Tick #{}: {:.2}ms total | spawn={:.2}ms ai={:.2}ms move={:.2}ms collision={:.2}ms decay={:.2}ms broadcast={:.2}ms | {} entities, {} players",
                self.tick_count,
                total_time.as_secs_f64() * 1000.0,
                spawn_time.as_secs_f64() * 1000.0,
                ai_time.as_secs_f64() * 1000.0,
                movement_time.as_secs_f64() * 1000.0,
                collision_time.as_secs_f64() * 1000.0,
                decay_time.as_secs_f64() * 1000.0,
                broadcast_time.as_secs_f64() * 1000.0,
                entity_count,
                player_count
            );
        }

        PendingBroadcasts {
            world_update: Some(world_broadcast),
            leaderboard: leaderboard_broadcast,
            xray_messages,
        }
    }

    /// Prepare the world state broadcast data.
    fn prepare_world_broadcast(&mut self) -> (WorldUpdateBroadcast, Vec<TargetedMessage>) {
        // Build cell list using pooled buffer
        self.broadcast_world_cells.clear();
        for (&node_id, entry) in self.world.iter_cells() {
            let data = entry.data();
            let (name, skin, owner_id) = if let CellEntry::Player(_p) = entry {
                let owner_id = data.owner_id;
                let (name, skin) = if let Some(oid) = owner_id {
                    if let Some(client) = self.clients.get(&oid) {
                        (
                            if client.name.is_empty() {
                                None
                            } else {
                                Some(client.name.clone())
                            },
                            client.skin.clone(),
                        )
                    } else if let Some(bot) = self.bots.get_bot(oid) {
                        // Check if it's a bot/minion
                        (
                            if bot.name.is_empty() {
                                None
                            } else {
                                Some(bot.name.clone())
                            },
                            None, // Bots don't have skins
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                (name, skin, owner_id)
            } else {
                (None, None, None)
            };

            // Mother cells use cell type 2 (Virus) for protocol compatibility
            // (JS MotherCell extends Virus and has cellType = 2)
            let protocol_cell_type = match data.cell_type {
                crate::entity::CellType::MotherCell => 2,
                other => other as u8,
            };

            self.broadcast_world_cells.push(WorldCell {
                node_id,
                x: data.position.x,
                y: data.position.y,
                size: data.size,
                color: data.color,
                cell_type: protocol_cell_type,
                name,
                skin,
                owner_id,
            });
        }

        // Build per-client data
        let mut client_data = HashMap::new();
        for (&client_id, client) in &self.clients {
            if !client.handshake_complete {
                continue;
            }

            // Calculate center position from owned cells
            let (center_x, center_y, total_size) = if client.cells.is_empty() {
                (client.center_x, client.center_y, 0.0)
            } else {
                let mut cx = 0.0;
                let mut cy = 0.0;
                let mut total = 0.0;
                for &cell_id in &client.cells {
                    if let Some(cell) = self.world.get_cell(cell_id) {
                        let data = cell.data();
                        cx += data.position.x;
                        cy += data.position.y;
                        total += data.size;
                    }
                }
                let count = client.cells.len() as f32;
                (cx / count, cy / count, total)
            };

            // Calculate scale based on total size
            let scale = if total_size <= 0.0 {
                1.0
            } else {
                (64.0 / total_size).min(1.0).powf(0.4)
            };

            client_data.insert(
                client_id,
                ClientViewData {
                    center_x,
                    center_y,
                    scale,
                    cell_ids: client.cells.clone(),
                    minion_ids: client.minions.clone(),
                    protocol: client.protocol,
                    scramble_id: client.scramble_id,
                    scramble_x: client.scramble_x,
                    scramble_y: client.scramble_y,
                    name: client.name.clone(),
                    skin: client.skin.clone(),
                },
            );
        }

        // Build broadcast
        let world_broadcast = WorldUpdateBroadcast {
            cells: self.broadcast_world_cells.clone(),
            eaten: self.eaten_this_tick.clone(),
            removed: Vec::new(), // TODO: track removed cells
            client_data,
        };

        // Prepare XRay data for clients that have it enabled
        let xray_messages = self.prepare_xray_data();
        
        (world_broadcast, xray_messages)
    }

    /// Prepare XRay data for all clients with xray_enabled=true.
    fn prepare_xray_data(&mut self) -> Vec<TargetedMessage> {
        let mut messages = Vec::new();
        // Find all clients that have XRay enabled using pooled buffer
        self.xray_client_ids.clear();
        for (&id, c) in &self.clients {
            if c.xray_enabled && c.is_operator {
                self.xray_client_ids.push(id);
            }
        }

        if self.xray_client_ids.is_empty() {
            return messages;
        }

        // Collect all player cells from all clients (excluding self)
        for i in 0..self.xray_client_ids.len() {
            let xray_client_id = self.xray_client_ids[i];
            let mut player_cells = Vec::new();

            // Get the XRay client's scramble values
            let (scramble_id, scramble_x, scramble_y) = match self.clients.get(&xray_client_id) {
                Some(c) => (c.scramble_id, c.scramble_x, c.scramble_y),
                None => continue,
            };

            // Iterate through all clients and collect their player cells
            for (&client_id, client) in &self.clients {
                // Don't include self
                if client_id == xray_client_id {
                    continue;
                }

                // Skip spectators and clients with no cells
                if client.is_spectating || client.cells.is_empty() {
                    continue;
                }

                // Get player name
                let name = if client.name.is_empty() {
                    "An unnamed cell".to_string()
                } else {
                    client.name.clone()
                };

                // Collect all player cells (type 0)
                for &cell_id in &client.cells {
                    if let Some(cell) = self.world.get_cell(cell_id) {
                        let data = cell.data();
                        // Only include player cells (type 0)
                        if data.cell_type == CellType::Player {
                            player_cells.push(protocol::packets::XrayPlayerCell {
                                node_id: data.node_id,
                                x: data.position.x as i32,
                                y: data.position.y as i32,
                                size: data.size as u16,
                                color: client.color,
                                name: name.clone(),
                            });
                        }
                    }
                }
            }

            // Also include bot cells (but exclude minions)
            // Collect all minion IDs to filter them out
            let minion_ids: std::collections::HashSet<u32> = self.clients.values()
                .flat_map(|c| &c.minions)
                .copied()
                .collect();

            for bot in &self.bots.bots {
                // Skip minions - they shouldn't appear in XRay
                if minion_ids.contains(&bot.id) {
                    continue;
                }

                let name = if bot.name.is_empty() {
                    "[BOT]".to_string()
                } else {
                    bot.name.clone()
                };

                for &cell_id in &bot.cells {
                    if let Some(cell) = self.world.get_cell(cell_id) {
                        let data = cell.data();
                        // Only include player cells (type 0)
                        if data.cell_type == CellType::Player {
                            player_cells.push(protocol::packets::XrayPlayerCell {
                                node_id: data.node_id,
                                x: data.position.x as i32,
                                y: data.position.y as i32,
                                size: data.size as u16,
                                color: bot.color,
                                name: name.clone(),
                            });
                        }
                    }
                }
            }

            // Prepare XRay packet
            messages.push(TargetedMessage {
                client_id: xray_client_id,
                message: TargetedMessageType::XrayData {
                    player_cells,
                    scramble_id,
                    scramble_x,
                    scramble_y,
                },
            });
        }
        
        messages
    }

    /// Prepare the leaderboard broadcast data.
    fn prepare_leaderboard_broadcast(&self) -> LeaderboardBroadcast {
        let entries = self.gamemode.get_leaderboard(&self.world, &self.clients, &self.bots);
        
        LeaderboardBroadcast { 
            entries,
            gamemode_id: self.gamemode.id(),
            gamemode_name: self.gamemode.name().to_string(),
        }
    }

    /// Update cells that are moving (boosted).
    fn update_moving_cells(&mut self) {
        let border_min = glam::Vec2::new(
            self.world.border.min_x,
            self.world.border.min_y,
        );
        let border_max = glam::Vec2::new(
            self.world.border.max_x,
            self.world.border.max_y,
        );

        // Collect cells that stopped moving
        let mut to_remove: Vec<u32> = Vec::new();

        for i in 0..self.world.moving_cells.len() {
            let cell_id = self.world.moving_cells[i];
            let still_moving = if let Some(cell) = self.world.get_cell_mut(cell_id) {
                cell.data_mut().update_boost(border_min, border_max)
            } else {
                false
            };

            // Update position in spatial index
            self.world.update_cell_position(cell_id);

            if !still_moving {
                to_remove.push(cell_id);
            }
        }

        // Remove stopped cells using O(1) removal
        for cell_id in to_remove {
            self.world.remove_from_moving(cell_id);
        }
    }

    /// Update player cell movement toward mouse.
    fn update_player_movement(&mut self) {
        // Copy border values to avoid borrow conflicts
        let border_min_x = self.world.border.min_x;
        let border_min_y = self.world.border.min_y;
        let border_max_x = self.world.border.max_x;
        let border_max_y = self.world.border.max_y;
        let speed_config = self.config.player.speed;

        // Reuse pooled buffer - clear and rebuild
        self.movement_cell_targets.clear();
        for client in self.clients.values() {
            if client.frozen || client.cells.is_empty() {
                continue;
            }
            let mx = client.mouse_x as f32;
            let my = client.mouse_y as f32;
            for &cell_id in &client.cells {
                self.movement_cell_targets.push((cell_id, mx, my, client.id));
            }
        }

        // Pre-compute speed multipliers per owner (avoids repeated gamemode calls)
        self.movement_speed_mults.clear();
        for &(_, _, _, owner_id) in &self.movement_cell_targets {
            self.movement_speed_mults.entry(owner_id).or_insert_with(|| self.gamemode.get_speed_multiplier(owner_id));
        }

        // Move data out temporarily to avoid borrow issues
        let mut cell_targets = std::mem::take(&mut self.movement_cell_targets);
        let speed_mults = std::mem::take(&mut self.movement_speed_mults);

        for (cell_id, mouse_x, mouse_y, owner_id) in cell_targets.drain(..) {
            if let Some(cell) = self.world.get_cell_mut(cell_id) {
                let data = cell.data_mut();

                // Calculate direction to mouse
                let dx = mouse_x - data.position.x;
                let dy = mouse_y - data.position.y;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist < 1.0 {
                    continue;
                }

                // Calculate speed based on size, with gamemode multiplier
                let base_speed = 2.2 * data.size.powf(-0.439) * 40.0;
                let gm_mult = speed_mults.get(&owner_id).copied().unwrap_or(1.0);
                let speed = base_speed * (speed_config as f32 / 30.0) * (dist.min(32.0) / 32.0) * gm_mult;

                // Normalize and apply movement
                let move_x = (dx / dist) * speed;
                let move_y = (dy / dist) * speed;

                data.position.x += move_x;
                data.position.y += move_y;

                // Clamp to border
                data.check_border(border_min_x, border_min_y, border_max_x, border_max_y);
            }
        }

        // Restore buffers for next tick (already drained/cleared, ready for reuse)
        self.movement_cell_targets = cell_targets;
        self.movement_speed_mults = speed_mults;
    }

    /// Process collisions between cells.
    fn process_collisions(&mut self) {
        use crate::collision::{check_cell_collision, size_to_mass};
        use crate::entity::CellType;

        // Clear and reuse buffers instead of allocating new ones
        self.collision_owner_lookup.clear();
        self.collision_remerge_lookup.clear();
        self.collision_eat_events.clear();
        self.collision_cells_to_remove.clear();
        self.collision_virus_pops.clear();
        self.collision_virus_ate_eject.clear();

        // Build owner lookup and can_remerge lookup
        for (&client_id, client) in &self.clients {
            for &cell_id in &client.cells {
                self.collision_owner_lookup.insert(cell_id, client_id);
                // Get canRemerge from the actual cell
                if let Some(CellEntry::Player(cell)) = self.world.get_cell(cell_id) {
                    self.collision_remerge_lookup.insert(cell_id, cell.can_remerge);
                } else {
                    self.collision_remerge_lookup.insert(cell_id, true);
                }
            }
        }

        // Add bots to lookups
        for bot in &self.bots.bots {
            for &cell_id in &bot.cells {
                self.collision_owner_lookup.insert(cell_id, bot.id);
                if let Some(CellEntry::Player(cell)) = self.world.get_cell(cell_id) {
                    self.collision_remerge_lookup.insert(cell_id, cell.can_remerge);
                } else {
                    self.collision_remerge_lookup.insert(cell_id, true);
                }
            }
        }

        // Process each player cell for eating
        let player_count = self.world.player_cells.len();

        for i in 0..player_count {
            let cell_id = self.world.player_cells[i];
            // Get cell data
            let (cell_pos, cell_size, cell_type_val) = match self.world.get_cell(cell_id) {
                Some(cell) => {
                    let data = cell.data();
                    (data.position, data.size, data.cell_type)
                }
                None => continue,
            };

            let cell_owner = self.collision_owner_lookup.get(&cell_id).copied();
            let cell_age = {
                if let Some(cell) = self.world.get_cell(cell_id) {
                    self.tick_count.saturating_sub(cell.data().tick_of_birth)
                } else {
                    0
                }
            };

            // Find nearby cells using QuadTree
            // Use a larger radius to ensure we find entities that we might be overlapping with
            let search_radius = (cell_size * 3.0).max(cell_size + 200.0);
            let nearby = self.world.find_cells_in_radius(cell_pos.x, cell_pos.y, search_radius);

            for &check_id in &nearby {
                if check_id == cell_id {
                    continue;
                }

                // Skip already removed cells - bitset O(1) check
                let check_id_idx = check_id as usize;
                let cell_id_idx = cell_id as usize;
                if (check_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(check_id_idx))
                    || (cell_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(cell_id_idx)) {
                    continue;
                }

                let (check_pos, check_size, check_type, check_age) = match self.world.get_cell(check_id) {
                    Some(c) => {
                        let data = c.data();
                        let age = self.tick_count.saturating_sub(data.tick_of_birth);
                        (data.position, data.size, data.cell_type, age)
                    }
                    None => continue,
                };

                // Check collision
                let collision = check_cell_collision(
                    cell_pos,
                    cell_size,
                    check_pos,
                    check_size,
                    cell_id,
                    check_id,
                );

                if !collision.is_colliding() {
                    continue;
                }

                // JS logic: swap so smaller cell is "cell" and larger is "check" (the eater)
                // This ensures the larger cell always eats the smaller one
                let (smaller_id, smaller_size, smaller_owner, smaller_age, smaller_type) =
                    if cell_size > check_size {
                        (check_id, check_size, self.collision_owner_lookup.get(&check_id).copied(), check_age, check_type)
                    } else {
                        (cell_id, cell_size, cell_owner, cell_age, cell_type_val)
                    };
                let (larger_id, larger_size, larger_owner, larger_age, larger_type) =
                    if cell_size > check_size {
                        (cell_id, cell_size, cell_owner, cell_age, cell_type_val)
                    } else {
                        (check_id, check_size, self.collision_owner_lookup.get(&check_id).copied(), check_age, check_type)
                    };

                // Skip if either is already removed
                let smaller_id_idx = smaller_id as usize;
                let larger_id_idx = larger_id as usize;
                if (smaller_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(smaller_id_idx))
                    || (larger_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(larger_id_idx)) {
                    continue;
                }

                // Check actual overlap threshold
                // JS resolveCollision: size = check._size - cell._size / div
                // (check = larger, cell = smaller; applies to ALL cell types)
                let div = if self.config.server.mobile_physics { 20.0 } else { 3.0 };
                let eat_threshold = larger_size - smaller_size / div;

                if collision.squared >= eat_threshold * eat_threshold {
                    continue; // Not overlapping enough to eat
                }

                // JS line 741: if (cell.cellType === 3 && cell.getAge() < 1) return;
                // Ejected mass must survive at least one full tick before it can be eaten.
                // Rust increments tick_count at the start of tick(), so freshly spawned
                // ejects (born at tick N) will have age 1 on their first collision check;
                // use < 2 to match the one-tick grace window of the JS version.
                if smaller_type == CellType::EjectedMass && smaller_age < 2 {
                    continue;
                }

                // JS: if (!check.canEat(cell)) return;   (check = larger)
                // canEat per JS entity class:
                //   Food / EjectedMass  → false  (base Cell)
                //   Virus               → cell.cellType === 3  (eject only), AND virus count < max
                //   PlayerCell          → true
                //   MotherCell          → handled by special case below
                match larger_type {
                    CellType::Food | CellType::EjectedMass => {
                        continue; // these types can never eat
                    }
                    CellType::Virus => {
                        // Virus.canEat only returns true for ejected mass when under max
                        if smaller_type != CellType::EjectedMass
                            || self.world.virus_cells.len() >= self.config.virus.max_amount
                        {
                            continue;
                        }
                    }
                    _ => {} // Player, MotherCell – proceed to detailed checks
                }

                // Now check if the LARGER cell can eat the SMALLER cell
                let can_eat_check = match smaller_type {
                    CellType::Food => true,
                    CellType::EjectedMass => true,
                    CellType::MotherCell | CellType::Virus => {
                        // Larger cell can eat virus if it's bigger
                        larger_size > smaller_size
                    }
                    CellType::Player => {
                        if smaller_owner == larger_owner && smaller_owner.is_some() {
                            // Same owner - check merge cooldown
                            let smaller_can_remerge = self.collision_remerge_lookup.get(&smaller_id).copied().unwrap_or(false);
                            let larger_can_remerge = self.collision_remerge_lookup.get(&larger_id).copied().unwrap_or(false);

                            // Both cells must be able to remerge AND be old enough
                            let split_restore_ticks = if self.config.server.mobile_physics { 1 } else { 13 };
                            let can_merge = smaller_can_remerge && larger_can_remerge &&
                                           smaller_age >= split_restore_ticks && larger_age >= split_restore_ticks;
                            // For equal sizes, use ID as tiebreaker
                            can_merge && (larger_size > smaller_size || (larger_size == smaller_size && larger_id > smaller_id))
                        } else {
                            // Different owners - check if larger can eat smaller
                            let gamemode_allows = self.gamemode.can_eat(
                                larger_owner.unwrap_or(0), 
                                smaller_owner.unwrap_or(0), 
                                &self.clients, 
                                &self.bots
                            );
                            if gamemode_allows {
                                // JS: check._size < mult * cell._size (where check is eater/larger, cell is food/smaller)
                                // Inverted: larger_size >= mult * smaller_size
                                let mult = 1.15; // playerEatMult
                                let size_check = larger_size >= mult * smaller_size;
                                size_check
                            } else {
                                false
                            }
                        }
                    }
                };
            
                // Special case: MotherCell can eat players
                if smaller_type == CellType::Player && larger_type == CellType::MotherCell {
                     // MotherCell (larger) eats player (smaller) if player is smaller/same size
                     let eaten_mass = crate::collision::size_to_mass(smaller_size);
                     self.collision_eat_events.push((larger_id, smaller_id, eaten_mass));
                     let idx = smaller_id as usize;
                     if idx >= self.collision_cells_to_remove.len() {
                         self.collision_cells_to_remove.grow(idx + 1);
                     }
                     self.collision_cells_to_remove.insert(idx);
                     continue;
                }

                if can_eat_check {
                    // Larger cell eats smaller cell
                    let eaten_mass = size_to_mass(smaller_size);
                    self.collision_eat_events.push((larger_id, smaller_id, eaten_mass));
                    let idx = smaller_id as usize;
                    if idx >= self.collision_cells_to_remove.len() {
                        self.collision_cells_to_remove.grow(idx + 1);
                    }
                    self.collision_cells_to_remove.insert(idx);
                    
                    // Check if player ate a virus - trigger pop
                    if larger_type == CellType::Player && smaller_type == CellType::Virus {
                        // Store virus pop event: (owner_id, player_cell_id)
                        if let Some(owner_id) = larger_owner {
                            self.collision_virus_pops.push((owner_id, larger_id));
                        }
                    }
                }
            }
        }

        // Moving-cells collision pass: mirrors JS nodesMoving loop.
        // The player-cells loop above only iterates player cells as the "primary"
        // cell, so virus-vs-eject collisions are missed when no player cell is
        // nearby.  Here we scan every moving virus and every moving eject for
        // the other half of the pair.
        // Virus.onEat(eject) behaviour: grow, and shoot a new virus if it
        // reaches virusMaxSize.
        {
            let virus_count = self.world.virus_cells.len();
            let virus_max = self.config.virus.max_amount;

            let moving_snapshot: Vec<u32> = self.world.moving_cells.clone();
            for &cell_id in &moving_snapshot {
                let cell_id_idx = cell_id as usize;
                if cell_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(cell_id_idx) {
                    continue;
                }

                let (cell_pos, cell_size, cell_type) = match self.world.get_cell(cell_id) {
                    Some(cell) => {
                        let d = cell.data();
                        (d.position, d.size, d.cell_type)
                    }
                    None => continue,
                };

                // Only interested in Virus or EjectedMass moving cells
                if cell_type != CellType::Virus && cell_type != CellType::EjectedMass {
                    continue;
                }

                let search_radius = (cell_size * 3.0).max(cell_size + 200.0);
                let nearby = self.world.find_cells_in_radius(cell_pos.x, cell_pos.y, search_radius);

                for &check_id in &nearby {
                    let check_id_idx = check_id as usize;
                    let cell_id_idx = cell_id as usize;
                    if check_id == cell_id
                        || (check_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(check_id_idx))
                        || (cell_id_idx < self.collision_cells_to_remove.len() && self.collision_cells_to_remove.contains(cell_id_idx))
                    {
                        continue;
                    }

                    let (check_pos, check_size, check_type, check_age) = match self.world.get_cell(check_id) {
                        Some(c) => {
                            let d = c.data();
                            let age = self.tick_count.saturating_sub(d.tick_of_birth);
                            (d.position, d.size, d.cell_type, age)
                        }
                        None => continue,
                    };

                    // Identify virus and eject in the pair (either order)
                    let (virus_id, virus_size, virus_pos, eject_id, eject_size, eject_pos, eject_age) =
                        if cell_type == CellType::Virus && check_type == CellType::EjectedMass {
                            (cell_id, cell_size, cell_pos, check_id, check_size, check_pos, check_age)
                        } else if cell_type == CellType::EjectedMass && check_type == CellType::Virus {
                            (check_id, check_size, check_pos, cell_id, cell_size, cell_pos, {
                                if let Some(c) = self.world.get_cell(cell_id) {
                                    self.tick_count.saturating_sub(c.data().tick_of_birth)
                                } else { 0 }
                            })
                        } else {
                            continue; // not a virus-eject pair
                        };

                    // Virus can only eat when under max count
                    if virus_count >= virus_max {
                        continue;
                    }

                    // Ejected-mass age grace window (same as player-cells loop)
                    if eject_age < 2 {
                        continue;
                    }

                    // Collision + overlap check
                    let collision = check_cell_collision(virus_pos, virus_size, eject_pos, eject_size, virus_id, eject_id);
                    if !collision.is_colliding() {
                        continue;
                    }
                    let (larger_size, smaller_size) = if virus_size > eject_size {
                        (virus_size, eject_size)
                    } else {
                        (eject_size, virus_size)
                    };
                    let div = if self.config.server.mobile_physics { 20.0 } else { 3.0 };
                    let eat_threshold = larger_size - smaller_size / div;
                    if collision.squared >= eat_threshold * eat_threshold {
                        continue;
                    }

                    // Virus eats ejected mass – growth uses the same on_eat formula;
                    // after applying, we check whether the virus hit virusMaxSize
                    // and needs to shoot (handled after the eat-event loop).
                    self.collision_eat_events.push((virus_id, eject_id, size_to_mass(eject_size)));
                    self.collision_virus_ate_eject.push(virus_id);
                    let idx = eject_id as usize;
                    if idx >= self.collision_cells_to_remove.len() {
                        self.collision_cells_to_remove.grow(idx + 1);
                    }
                    self.collision_cells_to_remove.insert(idx);
                }
            }
        }

        // Handle rigid collisions for same-owner cells that can't merge yet
        self.process_rigid_collisions();

        // Apply eat events
        for (eater_id, eaten_id, eaten_mass) in &self.collision_eat_events {
            // Track for client updates
            self.eaten_this_tick.push((*eaten_id, *eater_id));

            // Add mass to eater using on_eat method
            // eaten_mass is size²/100, multiply by 100 to get radius (size²)
            if let Some(eater) = self.world.get_cell_mut(*eater_id) {
                let data = eater.data_mut();
                data.on_eat(*eaten_mass * 100.0);
            }

            // Update QuadTree for eater
            self.world.update_cell_position(*eater_id);
        }

        // Virus onEat post-processing: if a virus that ate an eject grew past
        // virusMaxSize, reset it to virusMinSize and shoot a new virus in the
        // direction the eaten eject was travelling.
        // JS Virus.onEat: setSize(virusMinSize); shootVirus(this, cell.boostDirection.angle)
        {
            let virus_max_size = self.config.virus.max_size as f32;
            let virus_min_size = self.config.virus.min_size as f32;
            let virus_eject_speed = self.config.virus.eject_speed as f32;

            for &vid in &self.collision_virus_ate_eject {
                let virus_size = match self.world.get_cell(vid) {
                    Some(c) => c.data().size,
                    None => continue,
                };
                if virus_size < virus_max_size {
                    continue;
                }
                // Reset virus to min size
                if let Some(c) = self.world.get_cell_mut(vid) {
                    c.data_mut().set_size(virus_min_size);
                }
                self.world.update_cell_position(vid);

                // Shoot a new virus from this position in a random direction
                // (JS uses the eaten eject's boostDirection.angle; we use a
                // random angle here as the eject is already removed and its
                // boost data is gone).
                let virus_pos = match self.world.get_cell(vid) {
                    Some(c) => c.data().position,
                    None => continue,
                };
                let mut rng = rand::rng();
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let new_virus_id = self.world.next_id();
                let mut new_virus = crate::entity::Virus::new(new_virus_id, virus_pos, virus_min_size, self.tick_count);
                new_virus.data_mut().set_boost(virus_eject_speed, angle);
                self.world.add_virus(new_virus);
                self.world.add_moving(new_virus_id);
            }
        }

        // Remove eaten cells - batch remove from client lists first
        if self.collision_cells_to_remove.count_ones(..) > 0 {
            // Build HashSet from bitset indices for efficient contains checks in retain
            let cells_to_remove_set: std::collections::HashSet<u32> = self.collision_cells_to_remove.ones()
                .map(|idx| idx as u32)
                .collect();
            
            for client in self.clients.values_mut() {
                client.cells.retain(|id| !cells_to_remove_set.contains(id));
            }
            // Remove from bots too
            for bot in &mut self.bots.bots {
                bot.cells.retain(|id| !cells_to_remove_set.contains(id));
            }

            // Detect deaths: clients/bots that now have zero cells
            // Build victim→killer map from eat_events using owner_lookup
            let mut victim_killer: HashMap<u32, u32> = HashMap::new();
            for &(eater_id, eaten_id, _) in &self.collision_eat_events {
                let eater_owner = self.collision_owner_lookup.get(&eater_id).copied().unwrap_or(0);
                let eaten_owner = self.collision_owner_lookup.get(&eaten_id).copied().unwrap_or(0);
                if eater_owner != 0 && eaten_owner != 0 && eater_owner != eaten_owner {
                    victim_killer.entry(eaten_owner).or_insert(eater_owner);
                }
            }
            for (&victim_id, &killer_id) in &victim_killer {
                let is_dead = if let Some(c) = self.clients.get(&victim_id) {
                    c.cells.is_empty()
                } else if let Some(b) = self.bots.get_bot(victim_id) {
                    b.cells.is_empty()
                } else {
                    false
                };
                if is_dead {
                    self.deaths_this_tick.push((killer_id, victim_id));
                }
            }

            // Then remove from world
            for cell_id in self.collision_cells_to_remove.ones() {
                self.world.remove_cell(cell_id as u32);
            }
        }

        // Handle virus pops AFTER eating is done
        let virus_pops = std::mem::take(&mut self.collision_virus_pops);
        self.process_virus_pops(virus_pops);
    }

    /// Pop a player into multiple cells when they eat a virus.
    fn process_virus_pops(&mut self, virus_pops: Vec<(u32, u32)>) {
        for (owner_id, cell_id) in virus_pops {
            // Get the cell's current mass
            let cell_mass = if let Some(cell) = self.world.get_cell(cell_id) {
                cell.data().mass
            } else {
                continue;
            };

            // Calculate how many cells we can split into
            // JS: cellsLeft = (config.virusMaxCells || config.playerMaxCells) - cell.owner.cells.length
            let max_cells = self.config.virus.max_cells;
            let current_cell_count = if let Some(client) = self.clients.get(&owner_id) {
                client.cells.len()
            } else if let Some(bot) = self.bots.get_bot(owner_id) {
                bot.cells.len()
            } else {
                continue;
            };

            let cells_left = max_cells.saturating_sub(current_cell_count);
            if cells_left == 0 {
                continue;
            }

            // JS: splitMin = config.virusSplitDiv
            let split_min = self.config.virus.split_div as f32;
            let splits = self.calculate_virus_splits(cell_mass, cells_left, split_min);

            // Split the player cell
            for &split_mass in &splits {
                let angle = rand::rng().random::<f32>() * std::f32::consts::TAU;
                self.split_player_cell_with_mass(owner_id, cell_id, angle, split_mass);
            }
        }
    }

    /// Calculate virus split masses.  Faithful port of JS Virus.onEaten split logic.
    fn calculate_virus_splits(&self, cell_mass: f32, cells_left: usize, split_min: f32) -> Vec<f32> {
        let mut splits = Vec::new();

        // Doubling branch: average mass per slot < splitMin → power-of-2 split count.
        // JS: if (cellMass / cellsLeft < splitMin) { ... return explode(...) }
        if cell_mass / (cells_left as f32) < split_min {
            let mut split_count: usize = 2;
            let mut split_mass = cell_mass / split_count as f32;

            // JS: while (splitMass > splitMin && 2*splitCount < cellsLeft)
            //       splitMass = cellMass / (splitCount *= 2)
            while split_mass > split_min && 2 * split_count < cells_left {
                split_count *= 2;
                split_mass = cell_mass / split_count as f32;
            }

            // Divide evenly among splitCount+1 (original cell keeps one share)
            // JS: splitMass = cellMass / (splitCount + 1)
            split_mass = cell_mass / (split_count + 1) as f32;
            for _ in 0..split_count {
                splits.push(split_mass);
            }
            return splits;
        }

        // Normal branch: enough mass to fill all slots.
        // Ports the JS post-decrement loop faithfully, including the fall-through
        // after the inner fill (which accounts for the original cell's share).
        // JS:   let massLeft = cellMass / 2;  splitMass = cellMass / 2;
        //       while (cellsLeft-- > 0) {
        //           if (massLeft / cellsLeft < splitMin) {
        //               splitMass = massLeft / cellsLeft;
        //               while (cellsLeft-- > 0) splits.push(splitMass);
        //           }                                          // ← no break, falls through
        //           while (splitMass >= massLeft && cellsLeft > 0) splitMass /= 2;
        //           splits.push(splitMass);  massLeft -= splitMass;
        //       }
        let mut mass_left = cell_mass / 2.0;
        let mut split_mass = cell_mass / 2.0;
        let mut remaining = cells_left as i32; // signed: inner fill drives it to -1

        loop {
            // JS: while (cellsLeft-- > 0) — post-decrement
            if remaining <= 0 {
                break;
            }
            remaining -= 1;

            let rem_f = remaining as f32;

            // JS: if (massLeft / cellsLeft < splitMin)
            // When remaining == 0, rem_f == 0.0 → division yields +inf, condition false.
            if mass_left / rem_f < split_min {
                split_mass = mass_left / rem_f;
                // JS: while (cellsLeft-- > 0) splits.push(splitMass)
                while remaining > 0 {
                    remaining -= 1;
                    splits.push(split_mass);
                }
                // Fall through — matches JS (no break/continue here)
            }

            // JS: while (splitMass >= massLeft && cellsLeft > 0) splitMass /= 2
            while split_mass >= mass_left && remaining > 0 {
                split_mass /= 2.0;
            }

            splits.push(split_mass);
            mass_left -= split_mass;
        }

        splits
    }

    /// Notify gamemode of player deaths detected this tick.
    fn process_deaths(&mut self) {
        let deaths: Vec<(u32, u32)> = self.deaths_this_tick.drain(..).collect();
        
        // Temporarily take gamemode ownership to satisfy borrow checker
        let mut gamemode = std::mem::replace(&mut self.gamemode, Box::new(crate::gamemodes::ffa::Ffa::new()));
        
        for (killer_id, victim_id) in deaths {
            // Check if the victim is a minion owned by any player
            let is_minion = self.clients.values().any(|client| client.minions.contains(&victim_id));
            
            // Only notify gamemode if victim is not a minion
            if !is_minion {
                gamemode.on_player_death(self, killer_id, victim_id);
            }
        }
        
        self.gamemode = gamemode;
    }

    /// Process rigid collisions (push apart) for same-owner cells that can't merge.
    fn process_rigid_collisions(&mut self) {
        let split_restore_ticks = if self.config.server.mobile_physics { 1 } else { 13 };
        let tick = self.tick_count;

        // Cache border values before we start mutating
        let border_min_x = self.world.border.min_x;
        let border_min_y = self.world.border.min_y;
        let border_max_x = self.world.border.max_x;
        let border_max_y = self.world.border.max_y;

        // Use index iteration to avoid cloning
        let player_count = self.world.player_cells.len();

        // Apply multiple passes for high-pressure situations (helps when many cells are squished)
        let max_passes = 2;
        for _pass in 0..max_passes {
            for i in 0..player_count {
            let cell_id = self.world.player_cells[i];
            
            // Re-read cell position each time (may have been updated by previous collisions)
            let (cell_pos, cell_size, cell_mass, cell_birth) = match self.world.get_cell(cell_id) {
                Some(cell) => {
                    let data = cell.data();
                    (data.position, data.size, data.mass, data.tick_of_birth)
                }
                None => continue,
            };

            let cell_owner = self.collision_owner_lookup.get(&cell_id).copied();
            let cell_age = tick.saturating_sub(cell_birth);
            let cell_can_remerge = self.collision_remerge_lookup.get(&cell_id).copied().unwrap_or(false);

            // Find nearby cells
            let nearby = self.world.find_cells_in_radius(cell_pos.x, cell_pos.y, cell_size * 2.0);

            for &check_id in &nearby {
                if check_id <= cell_id {
                    continue; // Avoid duplicate pairs
                }

                // Re-read check cell position (may have been updated)
                let (check_pos, check_size, check_mass, check_birth, check_type) = match self.world.get_cell(check_id) {
                    Some(c) => {
                        let data = c.data();
                        (data.position, data.size, data.mass, data.tick_of_birth, data.cell_type)
                    }
                    None => continue,
                };

                // Only apply rigid collision to player cells of same owner
                if check_type != crate::entity::CellType::Player {
                    continue;
                }

                let check_owner = self.collision_owner_lookup.get(&check_id).copied();
                if cell_owner != check_owner || cell_owner.is_none() {
                    continue;
                }

                let check_age = tick.saturating_sub(check_birth);
                let check_can_remerge = self.collision_remerge_lookup.get(&check_id).copied().unwrap_or(false);

                // Check if cells are too young or can't remerge - need rigid collision
                let needs_rigid = cell_age < split_restore_ticks
                    || check_age < split_restore_ticks
                    || !cell_can_remerge
                    || !check_can_remerge;

                if !needs_rigid {
                    continue; // Cells can merge, no rigid collision needed
                }

                // Check if cells are overlapping (JS: checkCellCollision + resolveRigidCollision)
                let collision = crate::collision::check_cell_collision(
                    cell_pos,
                    cell_size,
                    check_pos,
                    check_size,
                    cell_id,
                    check_id,
                );

                if !collision.is_colliding() {
                    continue; // Not overlapping
                }

                if collision.d < 0.01 {
                    continue; // Too close, avoid division by zero
                }

                // Distribute push based on mass ratio
                let total_mass = cell_mass + check_mass;
                if total_mass <= 0.0 {
                    continue;
                }
                let cell_ratio = check_mass / total_mass;
                let check_ratio = cell_mass / total_mass;

                // Calculate overlap depth ratio (0 = just touching, 1 = completely overlapping)
                let overlap_ratio = ((collision.r - collision.d) / collision.r).clamp(0.0, 1.0);
                
                // Scale force based on overlap depth:
                // - Very high overlap (>75%, just split): gentle 0.5x to prevent explosion
                // - High overlap (60-75%): ramp up from 0.8x to 1.0x
                // - Medium overlap (30-60%): stronger 1.0x to 2.0x to separate stuck cells
                // - Low overlap (<30%): maximum 2.0x to 3.5x for final separation
                let pressure_multiplier = if overlap_ratio > 0.75 {
                    // Just split: gentle
                    0.5 + (1.0 - overlap_ratio) * 2.0  // 0.5 at 100%, 1.0 at 75%
                } else if overlap_ratio > 0.6 {
                    // Ramping up
                    0.8 + (0.75 - overlap_ratio) * 1.4  // 1.0 at 75%, 0.8 at 60%
                } else if overlap_ratio > 0.3 {
                    // Moderate boost for stuck cells
                    0.9 + (0.6 - overlap_ratio) * 3.4  // 1.0 at 60%, 2.0 at 30%
                } else {
                    // Gentle push for separating cells
                    1.5 + overlap_ratio * 1.667  // 2.0 at 30%, 1.5 at 0% (just touching)
                };
                
                let adjusted_push = collision.push * pressure_multiplier;

                // Calculate direction (truncate to match JS ~~m.dx behavior)
                let fx = collision.dx.trunc();
                let fy = collision.dy.trunc();

                let push_x = fx * adjusted_push;
                let push_y = fy * adjusted_push;

                // Apply push to first cell immediately (matching JS behavior)
                if let Some(cell) = self.world.get_cell_mut(cell_id) {
                    let data = cell.data_mut();
                    data.position.x -= push_x * cell_ratio;
                    data.position.y -= push_y * cell_ratio;
                    // Keep within border
                    data.check_border(
                        border_min_x,
                        border_min_y,
                        border_max_x,
                        border_max_y,
                    );
                }
                self.world.update_cell_position(cell_id);

                // Apply push to second cell immediately
                if let Some(cell) = self.world.get_cell_mut(check_id) {
                    let data = cell.data_mut();
                    data.position.x += push_x * check_ratio;
                    data.position.y += push_y * check_ratio;
                    // Keep within border
                    data.check_border(
                        border_min_x,
                        border_min_y,
                        border_max_x,
                        border_max_y,
                    );
                }
                self.world.update_cell_position(check_id);
            }
        }
        }
    }

    /// Update merge status for all player cells.
    fn update_merge_status(&mut self) {
        let merge_time = self.config.player.merge_time;
        let tick = self.tick_count;

        // Update canRemerge for all player cells - iterate by index to avoid clone
        let player_count = self.world.player_cells.len();
        for i in 0..player_count {
            let cell_id = self.world.player_cells[i];
            if let Some(CellEntry::Player(cell)) = self.world.get_cell_mut(cell_id) {
                cell.update_merge(tick, merge_time as f32);
            }
        }
    }

    /// Update cell decay (large cells shrink).
    fn update_decay(&mut self) {
        let min_decay = self.config.player.min_size as f32;
        let decay_rate = self.config.player.decay_rate as f32;
        let decay_factor = 1.0 - decay_rate;

        // Collect cells to decay
        let mut decay_updates: Vec<(u32, f32)> = Vec::new();

        // Decay human player cells
        for (&_client_id, client) in &self.clients {
            for &cell_id in &client.cells {
                if let Some(cell) = self.world.get_cell(cell_id) {
                    let size = cell.data().size;
                    if size <= min_decay {
                        continue;
                    }

                    // Apply decay: size = sqrt(size^2 * (1 - rate))
                    // Optimized: sqrt(size^2 * decay) = size * sqrt(decay)
                    // Pre-compute sqrt(decay) since it's constant per tick
                    let new_size = size * decay_factor.sqrt();
                    let new_size = new_size.max(min_decay);

                    // Only update if change is significant (avoid tiny updates)
                    if size - new_size > 0.01 {
                        decay_updates.push((cell_id, new_size));
                    }
                }
            }
        }

        // Decay bot cells
        for bot in &self.bots.bots {
            for &cell_id in &bot.cells {
                if let Some(cell) = self.world.get_cell(cell_id) {
                    let size = cell.data().size;
                    if size <= min_decay {
                        continue;
                    }

                    let new_size = size * decay_factor.sqrt();
                    let new_size = new_size.max(min_decay);

                    if size - new_size > 0.01 {
                        decay_updates.push((cell_id, new_size));
                    }
                }
            }
        }

        // Apply decay updates
        for (cell_id, new_size) in decay_updates {
            if let Some(cell) = self.world.get_cell_mut(cell_id) {
                cell.data_mut().set_size(new_size);
            }
            self.world.update_cell_position(cell_id);
        }
    }

    /// Process bot respawns.
    fn process_bot_respawns(&mut self) {
        let start_size = self.config.player.start_size as f32;
        let tick_count = self.tick_count;

        // Get list of bots that need to respawn
        let respawn_list = self.bots.get_respawn_list();

        for bot_id in respawn_list {
            // Spawn a cell for this bot
            let position = self.world.border.random_position();
            let node_id = self.world.next_id();

            // Let GameMode handle team assignment if needed
            if let Some(bot) = self.bots.get_bot_mut(bot_id) {
                self.gamemode.on_bot_spawn(bot);
            }

            let (color, team) = if let Some(bot) = self.bots.get_bot(bot_id) {
                (bot.color, bot.team)
            } else {
                (crate::world::World::random_color(), None)
            };

            let mut cell = PlayerCell::new(node_id, bot_id, position, start_size as f32, tick_count);
            cell.cell_data.color = color;

            let cell_id = self.world.add_player_cell(cell);

            // Add to bot's cell list
            if let Some(bot) = self.bots.get_bot_mut(bot_id) {
                bot.cells.push(cell_id);
                bot.needs_respawn = false;
                debug!("Bot {} '{}' spawned cell {} in team {:?}", bot_id, bot.name, cell_id, team);
            }
        }
    }

    /// Process minion control: apply owner flags to minion bots.
    fn process_minions(&mut self) {
        // Collect minion actions from all clients
        let mut minion_targets: Vec<(u32, glam::Vec2, bool)> = Vec::new(); // (minion_id, target, frozen)
        let mut minion_splits: Vec<(u32, glam::Vec2)> = Vec::new(); // (minion_id, owner_mouse) - need mouse pos for split direction
        let mut minion_ejects: Vec<u32> = Vec::new();

        for client in self.clients.values_mut() {
            if !client.minion_control || client.minions.is_empty() {
                continue;
            }

            // Calculate owner center from their cells
            let owner_center = if client.cells.is_empty() {
                glam::Vec2::new(0.0, 0.0)
            } else {
                let mut cx = 0.0;
                let mut cy = 0.0;
                let mut count = 0;
                for &cell_id in &client.cells {
                    if let Some(cell) = self.world.get_cell(cell_id) {
                        let pos = cell.data().position;
                        cx += pos.x;
                        cy += pos.y;
                        count += 1;
                    }
                }
                if count > 0 { glam::Vec2::new(cx / count as f32, cy / count as f32) } else { glam::Vec2::new(0.0, 0.0) }
            };

            let owner_mouse = glam::Vec2::new(client.mouse_x as f32, client.mouse_y as f32);

            for &minion_id in &client.minions {
                if client.minion_frozen {
                    // Frozen minions don't move — set target to current position
                    if let Some(bot) = self.bots.get_bot(minion_id) {
                        if let Some(&cell_id) = bot.cells.first() {
                            if let Some(cell) = self.world.get_cell(cell_id) {
                                minion_targets.push((minion_id, cell.data().position, true));
                                continue;
                            }
                        }
                    }
                }

                if client.minion_collect {
                    // Seek nearest food/ejected mass
                    if let Some(bot) = self.bots.get_bot(minion_id) {
                        if let Some(&cell_id) = bot.cells.first() {
                            if let Some(cell) = self.world.get_cell(cell_id) {
                                let pos = cell.data().position;
                                let nearby = self.world.find_cells_in_radius(pos.x, pos.y, 500.0);
                                let mut best_target = if client.minion_follow { owner_center } else { owner_mouse };
                                let mut best_dist = f32::MAX;
                                for &nid in &nearby {
                                    if let Some(ncell) = self.world.get_cell(nid) {
                                        let ndata = ncell.data();
                                        if ndata.cell_type == crate::entity::CellType::Food || ndata.cell_type == crate::entity::CellType::EjectedMass {
                                            let dx = ndata.position.x - pos.x;
                                            let dy = ndata.position.y - pos.y;
                                            let dist = dx * dx + dy * dy;
                                            if dist < best_dist {
                                                best_dist = dist;
                                                best_target = ndata.position;
                                            }
                                        }
                                    }
                                }
                                minion_targets.push((minion_id, best_target, false));
                                continue;
                            }
                        }
                    }
                }

                // Default: follow center or mouse
                let target = if client.minion_follow { owner_center } else { owner_mouse };
                minion_targets.push((minion_id, target, false));
            }

            // Collect one-shot actions
            if client.minion_split {
                for &minion_id in &client.minions {
                    minion_splits.push((minion_id, owner_mouse));
                }
                client.minion_split = false;
            }
            if client.minion_eject {
                minion_ejects.extend_from_slice(&client.minions);
                client.minion_eject = false;
            }
        }

        // Apply targets to minion bots
        for (minion_id, target, _frozen) in minion_targets {
            if let Some(bot) = self.bots.get_bot_mut(minion_id) {
                bot.target = target;
            }
        }

        // Apply one-shot splits - set mouse target just before splitting
        for (minion_id, mouse_pos) in minion_splits {
            if let Some(bot) = self.bots.get_bot_mut(minion_id) {
                bot.target = mouse_pos;
            }
            self.handle_split(minion_id);
        }

        // Apply one-shot ejects
        for minion_id in minion_ejects {
            self.handle_eject(minion_id);
        }
    }

    /// Update bot cell movement toward their targets.
    fn update_bot_movement(&mut self) {
        // Copy border values to avoid borrow conflicts
        let border_min_x = self.world.border.min_x;
        let border_min_y = self.world.border.min_y;
        let border_max_x = self.world.border.max_x;
        let border_max_y = self.world.border.max_y;
        let speed_config = self.config.player.speed;

        // Collect (cell_id, target_x, target_y) tuples - avoids cloning cell vectors
        let mut cell_targets: Vec<(u32, f32, f32)> = Vec::with_capacity(64);
        for bot in &self.bots.bots {
            if !bot.cells.is_empty() {
                for &cell_id in &bot.cells {
                    cell_targets.push((cell_id, bot.target.x, bot.target.y));
                }
            }
        }

        for (cell_id, target_x, target_y) in cell_targets {
            if let Some(cell) = self.world.get_cell_mut(cell_id) {
                let data = cell.data_mut();

                // Calculate direction to target
                let dx = target_x - data.position.x;
                let dy = target_y - data.position.y;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist < 1.0 {
                    continue;
                }

                // Calculate speed based on size
                let base_speed = 2.2 * data.size.powf(-0.439) * 40.0;
                let speed = base_speed * (speed_config as f32 / 30.0) * (dist.min(32.0) / 32.0);

                // Normalize and apply movement
                let move_x = (dx / dist) * speed;
                let move_y = (dy / dist) * speed;

                data.position.x += move_x;
                data.position.y += move_y;

                // Clamp to border
                data.check_border(border_min_x, border_min_y, border_max_x, border_max_y);
            }
        }
    }

    /// Spawn initial bots based on config.
    pub fn spawn_bots(&mut self) {
        let bot_count = self.config.server.bots;
        if bot_count == 0 {
            return;
        }

        info!("Spawning {} bots", bot_count);
        for _ in 0..bot_count {
            let bot_id = self.bots.add_bot();
            debug!("Added bot {}", bot_id);
        }
    }
}

/// Parse player name and skin from the join string.
/// Format: `{skin}name` or just `name`.
fn parse_name_and_skin(input: &str) -> (Option<String>, String) {
    if input.starts_with('{') {
        if let Some(end) = input.find('}') {
            let skin = input[1..end].to_string();
            let name = input[end + 1..].to_string();
            return (Some(skin), name);
        }
    }
    (None, input.to_string())
}

/// Run the main game loop.
pub async fn run_game_loop(state: Arc<RwLock<GameState>>, tick_interval_ms: u64) {
    let start = Instant::now() + Duration::from_millis(tick_interval_ms);
    let mut ticker = interval_at(start, Duration::from_millis(tick_interval_ms));
    // Use Skip to catch up on missed ticks - ensures consistent game speed.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Initial spawn
    {
        let mut game = state.write().await;
        info!("Initial world spawn...");

        // Copy config values to avoid borrow conflicts
        let food_min = game.config.food.min_amount;
        let food_max = game.config.food.max_amount;
        let food_spawn = game.config.food.spawn_amount * 10; // Faster initial spawn
        let food_min_size = game.config.food.min_size as f32;
        let food_max_size = game.config.food.max_size as f32;
        let virus_min = game.config.virus.min_amount;
        let virus_max = game.config.virus.max_amount;
        let virus_size = game.config.virus.min_size as f32;

        // Spawn initial food
        game.world.spawn_food(food_min, food_max, food_spawn, food_min_size, food_max_size, 0);

        // Spawn initial viruses
        game.world.spawn_viruses(virus_min, virus_max, virus_size, 0);

        let counts = game.world.cell_counts();
        info!(
            "World initialized: {} food, {} viruses",
            counts.food, counts.viruses
        );

        // Spawn initial bots
        game.spawn_bots();
    }

    loop {
        let scheduled = ticker.tick().await;
        
        // Hibernate when no users are connected to reduce CPU usage
        {
            let game = state.read().await;
            if game.clients.is_empty() {
                drop(game);
                sleep(Duration::from_millis((tick_interval_ms * 4).max(100))).await;
                continue;
            }
        }
        
        // Drain any backlog of tick events so we always process the most recent tick.
        // This keeps user inputs up-to-date when the server falls behind.
        let mut skipped = 0u32;
        while ticker.tick().now_or_never().is_some() {
            skipped += 1;
        }
        if skipped > 0 {
            debug!("Skipped {} ticks to stay current (lag: {:?})", skipped, Instant::now().saturating_duration_since(scheduled));
        }
        
        // Run tick and extract pending broadcasts
        let broadcasts = {
            let mut game = state.write().await;
            let tick_start = std::time::Instant::now();
            let broadcasts = game.tick();
            let tick_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
            
            // Exponential moving average (weight 0.5, matches typical server stat smoothing)
            game.update_time_avg = game.update_time_avg * 0.5 + tick_ms * 0.5;
            
            // Warn if tick is too slow (>80% of tick interval = 20ms for 25ms interval)
            let tick_budget = tick_interval_ms as f64 * 0.9;
            if tick_ms > tick_budget {
                warn!(
                    "Slow tick #{}: {:.3}ms (budget: {:.1}ms) - {} players, {} cells total",
                    game.tick_count,
                    tick_ms,
                    tick_budget,
                    game.clients.len(),
                    game.world.cells.len()
                );
            }
            
            broadcasts
        }; // Write lock released here
        
        // Clone channel senders once with a single read lock
        let (world_tx, lb_tx, targeted_tx) = {
            let game = state.read().await;
            (game.world_tx.clone(), game.lb_tx.clone(), game.targeted_tx.clone())
        }; // Read lock released here
        
        // Send all broadcasts in parallel without any locks
        let _world_task = broadcasts.world_update.map(|world_update| {
            let tx = world_tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(world_update);
            })
        });
        
        let _lb_task = broadcasts.leaderboard.map(|leaderboard| {
            let tx = lb_tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(leaderboard);
            })
        });
        
        let _xray_task = if !broadcasts.xray_messages.is_empty() {
            let tx = targeted_tx.clone();
            let messages = broadcasts.xray_messages;
            Some(tokio::spawn(async move {
                for message in messages {
                    let _ = tx.send(message);
                }
            }))
        } else {
            None
        };
        
        // Optionally await all tasks (they're very fast, just channel sends)
        // if let Some(task) = world_task {
        //     let _ = task.await;
        // }
        // if let Some(task) = lb_task {
        //     let _ = task.await;
        // }
        // if let Some(task) = xray_task {
        //     let _ = task.await;
        // }
    }
}
