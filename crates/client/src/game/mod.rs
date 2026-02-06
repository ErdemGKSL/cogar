// Game state, cell management, world representation
use wasm_bindgen::prelude::*;
use glam::Vec2;
use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;
use web_sys::{window, HtmlCanvasElement, HtmlImageElement};
use js_sys::Math;
use protocol::BinaryReader;

use crate::network::Connection;
use crate::camera::Camera;
use crate::input::Input;
use crate::render::{Renderer, Minimap};
use crate::ui::UI;
use crate::utils;

// Performance: Compile-time constants for hot paths
const INTERPOLATION_DURATION_MS: f64 = 120.0;
const MOUSE_SEND_INTERVAL_MS: f64 = 40.0;
const FRAME_DT_MAX: f32 = 0.1;
const FADE_DURATION_MS: f64 = 120.0;
const DEATH_REMOVE_MS: f64 = 200.0;

/// Represents a cell in the game world.
///
/// Interpolation mirrors the JS client exactly:
///   dt  = clamp((now - update_time) / 120, 0, 1)
///   pos = (ox, oy)  +  (target - (ox, oy)) * dt
///   size = os + (target_size - os) * dt
#[derive(Clone)]
pub struct Cell {
    pub id: u32,
    /// Current interpolated position (what is actually drawn).
    pub position: Vec2,
    /// Server-supplied target position (nx, ny in JS).
    pub target_position: Vec2,
    /// Current interpolated size (what is actually drawn).
    pub size: f32,
    /// Smoothed render position (client-side animation).
    pub render_position: Vec2,
    /// Smoothed render size (client-side animation).
    pub render_size: f32,
    /// Jelly physics points (client-side rendering).
    pub points: Vec<RenderPoint>,
    /// Jelly physics point velocities.
    pub points_vel: Vec<f32>,
    /// Server-supplied target size (ns in JS).
    pub target_size: f32,
    /// Lerp-start position (ox, oy in JS) — snapped when a new server update arrives.
    pub ox: f32,
    pub oy: f32,
    /// Lerp-start size (os in JS).
    pub os: f32,
    pub color: (u8, u8, u8),
    pub name: String,
    pub skin: Option<String>,
    pub is_virus: bool,
    pub is_ejected: bool,
    pub is_food: bool,
    /// Timestamp (ms) when the most recent server update was received.
    pub update_time: f64,
    /// Timestamp when cell was born (for fade-in effect).
    pub born_time: f64,
    /// Timestamp when cell was destroyed/eaten (for fade-out effect).
    pub death_time: Option<f64>,
    /// ID of the cell that killed/ate this cell.
    pub killed_by: Option<u32>,
    /// Whether this cell has been destroyed and is animating out.
    pub is_destroyed: bool,
}

#[derive(Clone, serde::Deserialize)]
pub struct ServerStats {
    pub name: String,
    pub mode: String,
    pub uptime: u64,
    pub update: String,
    #[serde(rename = "playersTotal")]
    pub players_total: u32,
    #[serde(rename = "playersAlive")]
    pub players_alive: u32,
    #[serde(rename = "playersDead")]
    pub players_dead: u32,
    #[serde(rename = "playersSpect")]
    pub players_spect: u32,
    #[serde(rename = "botsTotal")]
    pub bots_total: u32,
    #[serde(rename = "playersLimit")]
    pub players_limit: u32,
}

#[derive(Clone, Copy)]
pub struct ClientSettings {
    pub show_skins: bool,
    pub show_names: bool,
    pub show_mass: bool,
    pub show_grid: bool,
    pub show_background_sectors: bool,
    pub show_minimap: bool,
    pub dark_theme: bool,
    pub jelly_physics: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            show_skins: true,
            show_names: true,
            show_mass: true,
            show_grid: true,
            show_background_sectors: true,
            show_minimap: true,
            dark_theme: true,
            jelly_physics: true,
        }
    }
}

impl Cell {
    pub fn new(id: u32, x: f32, y: f32, size: f32, color: (u8, u8, u8)) -> Self {
        let pos = Vec2::new(x, y);
        let now = utils::now();
        Self {
            id,
            position: pos,
            target_position: pos,
            size,
            target_size: size,
            render_position: pos,
            render_size: size,
            points: Vec::new(),
            points_vel: Vec::new(),
            ox: x,
            oy: y,
            os: size,
            color,
            name: String::new(),
            skin: None,
            is_virus: false,
            is_ejected: false,
            is_food: false,
            update_time: now,
            born_time: now,
            death_time: None,
            killed_by: None,
            is_destroyed: false,
        }
    }

    /// mass = size² / 100  (uses interpolated size, for on-cell label)
    #[inline]
    pub fn mass(&self) -> f32 {
        self.render_size * self.render_size / 100.0
    }

    /// Mark this cell as destroyed/eaten and start fade-out animation
    #[inline]
    pub fn destroy(&mut self, killer_id: Option<u32>) {
        self.is_destroyed = true;
        self.death_time = Some(utils::now());
        self.killed_by = killer_id;
    }

    /// Get the alpha (transparency) value for rendering based on birth/death animation
    #[inline]
    pub fn get_render_alpha(&self) -> f32 {
        let now = utils::now();
        
        if self.is_destroyed {
            // Fade out over 120ms
            if let Some(death_time) = self.death_time {
                let elapsed = now - death_time;
                ((FADE_DURATION_MS - elapsed).max(0.0) / FADE_DURATION_MS) as f32
            } else {
                1.0
            }
        } else {
            // Fade in over 120ms when born
            let elapsed = now - self.born_time;
            ((elapsed).min(FADE_DURATION_MS) / FADE_DURATION_MS) as f32
        }
    }

    /// Check if this cell should be removed from the game (200ms after death)
    #[inline]
    pub fn should_remove(&self) -> bool {
        if let Some(death_time) = self.death_time {
            utils::now() - death_time > DEATH_REMOVE_MS
        } else {
            false
        }
    }
}

/// The main game client state
#[wasm_bindgen]
pub struct GameClient {
    connection: Rc<RefCell<Connection>>,
    renderer: Renderer,
    minimap: Minimap,
    camera: Camera,
    input: Input,
    input_state: Rc<RefCell<Input>>,  // Shared with event handlers
    ui: UI,

    cells: HashMap<u32, Cell>,
    my_cells: Vec<u32>,
    border: (f32, f32, f32, f32), // min_x, min_y, max_x, max_y

    mouse_world_pos: Vec2,
    last_mouse_send: f64,
    last_update: f64,

    alive: bool,
    death_time: Option<f64>,  // When player died (for 250ms delay)
    pending_spawn_nick: Option<String>,
    pending_spawn: Rc<RefCell<Option<String>>>,  // Spawn request from button click
    last_nick: String,
    last_skin: Option<String>,

    leaderboard: Vec<(bool, String)>,

    /// Loaded skin images — key is the skin name, value is the (possibly still loading) Image element.
    skins: HashMap<String, HtmlImageElement>,

    // Packet queue - WebSocket handler pushes here, game loop processes
    packet_queue: Rc<RefCell<Vec<Vec<u8>>>>,

    // WebSocket event flags (to avoid borrow conflicts in event handlers)
    ws_open_flag: Rc<std::cell::Cell<bool>>,
    ws_close_flag: Rc<std::cell::Cell<bool>>,

    // FPS tracking
    frame_count: u32,
    last_fps_time: f64,
    fps: u32,
    saw_eat_record: bool,
    settings: ClientSettings,

    xray_players: Vec<XrayPlayer>,
    xray_last_update: f64,

    // Server stats
    server_stats: Option<ServerStats>,
    last_stats_request: f64,
    latency: Option<f64>,
}

#[derive(Clone, Copy)]
pub struct RenderPoint {
    pub x: f32,
    pub y: f32,
    pub rl: f32,
}

#[derive(Clone)]
struct XrayPlayer {
    id: u32,
    position: Vec2,
    size: f32,
    color: (u8, u8, u8),
    name: String,
}

#[derive(Clone, Copy)]
struct PointRef {
    x: f32,
    y: f32,
    parent_id: u32,
}

#[wasm_bindgen]
impl GameClient {
    pub fn new(canvas_id: &str, server_url: &str) -> Result<GameClient, JsValue> {
        let window = window().ok_or("No window")?;
        let document = window.document().ok_or("No document")?;
        let canvas = document
            .get_element_by_id(canvas_id)
            .ok_or("Canvas not found")?
            .dyn_into::<HtmlCanvasElement>()?;

        // Set canvas size
        canvas.set_width(window.inner_width()?.as_f64().unwrap() as u32);
        canvas.set_height(window.inner_height()?.as_f64().unwrap() as u32);

        let renderer = Renderer::new(canvas.clone())?;
        let minimap = Minimap::new()?;
        let connection = Connection::new(server_url)?;

        let conn_rc = Rc::new(RefCell::new(connection));

        let input_state = Rc::new(RefCell::new(Input::new()));
        let now = utils::now();
        let ui = UI::new(document);

        let client = Self {
            connection: conn_rc,
            renderer,
            minimap,
            camera: Camera::new(),
            input: Input::new(),
            input_state: input_state.clone(),
            ui,
            cells: HashMap::new(),
            my_cells: Vec::new(),
            border: (0.0, 0.0, 11180.0, 11180.0),
            mouse_world_pos: Vec2::ZERO,
            last_mouse_send: 0.0,
            last_update: now,
            alive: false,
            death_time: None,
            pending_spawn_nick: None,
            pending_spawn: Rc::new(RefCell::new(None)),
            last_nick: String::new(),
            last_skin: None,
            leaderboard: Vec::new(),
            skins: HashMap::new(),
            packet_queue: Rc::new(RefCell::new(Vec::new())),
            ws_open_flag: Rc::new(std::cell::Cell::new(false)),
            ws_close_flag: Rc::new(std::cell::Cell::new(false)),
            frame_count: 0,
            last_fps_time: now,
            fps: 0,
            saw_eat_record: false,
            settings: ClientSettings::default(),
            xray_players: Vec::new(),
            xray_last_update: 0.0,
            server_stats: None,
            last_stats_request: 0.0,
            latency: None,
        };

        Ok(client)
    }

    pub fn spawn(&mut self, nick: &str) {
        let (skin, name) = Self::parse_spawn_name(nick);
        self.last_nick = name;
        self.last_skin = skin;
        let spawn_name = self.build_spawn_name();
        self.pending_spawn_nick = Some(spawn_name.clone());
        if let Err(e) = self.connection.borrow().send_spawn(&spawn_name) {
            web_sys::console::error_1(&format!("Failed to send spawn: {:?}", e).into());
        }
    }

    pub fn websocket(&self) -> web_sys::WebSocket {
        self.connection.borrow().websocket().clone()
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn my_cells_count(&self) -> usize {
        self.my_cells.len()
    }

    pub fn send_chat_message(&self, message: &str) {
        if let Err(e) = self.connection.borrow().send_chat(message) {
            web_sys::console::error_1(&format!("Failed to send chat: {:?}", e).into());
        }
    }

    pub(crate) fn set_show_skins(&mut self, value: bool) {
        self.settings.show_skins = value;
    }

    pub(crate) fn set_show_names(&mut self, value: bool) {
        self.settings.show_names = value;
    }

    pub(crate) fn set_show_mass(&mut self, value: bool) {
        self.settings.show_mass = value;
    }

    pub(crate) fn set_show_grid(&mut self, value: bool) {
        self.settings.show_grid = value;
    }

    pub(crate) fn set_show_background_sectors(&mut self, value: bool) {
        self.settings.show_background_sectors = value;
    }

    pub(crate) fn set_show_minimap(&mut self, value: bool) {
        self.settings.show_minimap = value;
    }

    pub(crate) fn set_dark_theme(&mut self, value: bool) {
        self.settings.dark_theme = value;
        if let Some(document) = window().and_then(|w| w.document()) {
            if let Some(root) = document.document_element() {
                let theme = if value { "dark" } else { "light" };
                let _ = root.set_attribute("data-theme", theme);
            }
        }
    }

    pub(crate) fn adjust_zoom(&mut self, zoom_multiplier: f32) {
        self.camera.adjust_zoom_factor(zoom_multiplier);
    }
}

// Non-WASM methods (not exposed to JS)
impl GameClient {
    /// Get the packet queue (for WebSocket handler to push packets)
    pub(crate) fn packet_queue(&self) -> Rc<RefCell<Vec<Vec<u8>>>> {
        self.packet_queue.clone()
    }

    /// Get the input state (for event handlers to update)
    pub(crate) fn input_state(&self) -> Rc<RefCell<Input>> {
        self.input_state.clone()
    }

    pub(crate) fn handle_ws_open(&self) {
        let conn = self.connection.borrow();
        if let Err(e) = conn.send_protocol_version() {
            web_sys::console::error_1(&format!("Failed to send protocol: {:?}", e).into());
        }
        if let Err(e) = conn.send_handshake() {
            web_sys::console::error_1(&format!("Failed to send handshake: {:?}", e).into());
        }
        web_sys::console::log_1(&"WebSocket ready for spawn".into());
    }

    pub(crate) fn handle_disconnect(&mut self) {
        self.cells.clear();
        self.my_cells.clear();
        self.alive = false;
        self.death_time = Some(utils::now());
        self.xray_players.clear();
        self.xray_last_update = 0.0;
        
        // Immediately clear the canvas to remove old cells
        let background = if self.settings.dark_theme { "#111" } else { "#f2f2f2" };
        self.renderer.clear(background);
        
        self.ui.show_login_overlay(&self.last_nick, self.last_skin.as_deref());
    }

    pub(crate) fn reconnect(&mut self) -> Result<web_sys::WebSocket, JsValue> {
        self.connection.borrow_mut().reconnect()
    }

    /// Mother cell color (experimental mode).
    const MOTHER_COLOR: (u8, u8, u8) = (206, 99, 99);

    /// True if this cell should be considered a valid killer target.
    /// Only player cells and mother cells can be killers.
    fn is_potential_killer(cell: &Cell) -> bool {
        if cell.is_destroyed || cell.is_food || cell.is_ejected {
            return false;
        }

        if cell.is_virus {
            // Only mother cells (virus-colored) can kill/eat players.
            return cell.color == Self::MOTHER_COLOR;
        }

        true
    }

    /// True if this cell should be treated as a mother cell.
    fn is_mother_cell(cell: &Cell) -> bool {
        cell.is_virus && cell.color == Self::MOTHER_COLOR
    }

    /// Roughly mirror server eat rules for player-vs-player and mother-vs-player.
    fn can_potentially_eat(eaten_size: f32, eater_size: f32, eater_is_mother: bool) -> bool {
        if eater_is_mother {
            eater_size >= eaten_size
        } else {
            eater_size >= eaten_size * 1.15
        }
    }

    /// Find the position of the nearest viable cell to move toward when eaten
    /// Calculates distance from center to edge and only returns cells within reasonable range
    fn find_nearest_viable_target(
        eaten_pos: &Vec2, 
        eaten_size: f32, 
        available_cells: &std::collections::HashMap<u32, Vec2>,
        all_cells: &std::collections::HashMap<u32, Cell>
    ) -> Option<Vec2> {
        let mut best_within: Option<(f32, f32, Vec2)> = None;

        for (id, pos) in available_cells.iter() {
            // Get the target cell's size for edge calculation
            let target_cell = match all_cells.get(id) {
                Some(cell) => cell,
                None => continue,
            };

            let eater_is_mother = Self::is_mother_cell(target_cell);
            if !Self::can_potentially_eat(eaten_size, target_cell.render_size, eater_is_mother) {
                continue;
            }

            let center_to_center = eaten_pos.distance(*pos);
            let distance_to_edge = (center_to_center - target_cell.render_size).max(0.0);

            // Dynamic max distance: scaled by eaten size, with a small boost for larger eaters
            let max_distance = (eaten_size * 14.0).max(160.0).min(320.0)
                + (target_cell.render_size * 0.75).min(120.0);

            if distance_to_edge <= max_distance {
                let candidate = (distance_to_edge, center_to_center, *pos);
                if best_within
                    .as_ref()
                    .map(|(best_edge, best_center, _)| {
                        distance_to_edge < *best_edge
                            || (distance_to_edge == *best_edge && center_to_center < *best_center)
                    })
                    .unwrap_or(true)
                {
                    best_within = Some(candidate);
                }
            }

        }

        best_within.map(|(_, _, pos)| pos)
    }

    /// Optimized version that uses only sizes instead of full cell data
    fn find_nearest_viable_target_optimized(
        eaten_pos: &Vec2,
        eaten_size: f32,
        available_cells: &std::collections::HashMap<u32, Vec2>,
        cell_sizes: &std::collections::HashMap<u32, f32>,
        killer_is_mother: &std::collections::HashMap<u32, bool>
    ) -> Option<Vec2> {
        let mut best_within: Option<(f32, f32, Vec2)> = None;

        for (id, pos) in available_cells.iter() {
            // Get the target cell's size for edge calculation
            let target_size = match cell_sizes.get(id) {
                Some(size) => *size,
                None => continue,
            };

            let eater_is_mother = killer_is_mother.get(id).copied().unwrap_or(false);
            if !Self::can_potentially_eat(eaten_size, target_size, eater_is_mother) {
                continue;
            }

            let center_to_center = eaten_pos.distance(*pos);
            let distance_to_edge = (center_to_center - target_size).max(0.0);

            // Dynamic max distance: scaled by eaten size, with a small boost for larger eaters
            let max_distance = (eaten_size * 8.0).max(160.0).min(320.0)
                + (target_size * 0.35).min(120.0);

            if distance_to_edge <= max_distance {
                let candidate = (distance_to_edge, center_to_center, *pos);
                if best_within
                    .as_ref()
                    .map(|(best_edge, best_center, _)| {
                        distance_to_edge < *best_edge
                            || (distance_to_edge == *best_edge && center_to_center < *best_center)
                    })
                    .unwrap_or(true)
                {
                    best_within = Some(candidate);
                }
            }

        }

        best_within.map(|(_, _, pos)| pos)
    }

    /// Get the pending spawn queue (for spawn button)
    pub(crate) fn pending_spawn(&self) -> Rc<RefCell<Option<String>>> {
        self.pending_spawn.clone()
    }

    /// Get the WebSocket open flag (for WebSocket onopen handler)
    pub(crate) fn ws_open_flag(&self) -> Rc<std::cell::Cell<bool>> {
        self.ws_open_flag.clone()
    }

    /// Get the WebSocket close flag (for WebSocket onclose handler)
    pub(crate) fn ws_close_flag(&self) -> Rc<std::cell::Cell<bool>> {
        self.ws_close_flag.clone()
    }

    /// Start loading a skin image the first time it is encountered.
    /// The Image element is created immediately; the browser fetches the PNG asynchronously.
    /// Rendering checks `img.complete() && img.width() > 0` before drawing.
    fn ensure_skin_loaded(&mut self, skin_name: &str) {
        if self.skins.contains_key(skin_name) {
            return;
        }
        if let Ok(img) = HtmlImageElement::new() {
            img.set_src(&format!("./skins/{}.png", skin_name));
            self.skins.insert(skin_name.to_string(), img);
        }
    }

    /// Normalize skin names from the protocol or nick format.
    /// JS uses a leading '%' to indicate skins in protocol >= 11.
    fn normalize_skin_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized = trimmed.strip_prefix('%').unwrap_or(trimmed);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    }

    /// Parse `{skin}name` format used by the server.
    fn parse_spawn_name(input: &str) -> (Option<String>, String) {
        let trimmed = input.trim();
        if trimmed.starts_with('{') {
            if let Some(end) = trimmed.find('}') {
                let skin = trimmed[1..end].trim().to_string();
                let name = trimmed[end + 1..].trim().to_string();
                if !skin.is_empty() {
                    return (Some(skin), name);
                }
            }
        }
        (None, trimmed.to_string())
    }

    /// Build the spawn name with optional skin prefix.
    fn build_spawn_name(&self) -> String {
        if let Some(ref skin) = self.last_skin {
            if !skin.is_empty() {
                return format!("{{{}}}{}", skin, self.last_nick);
            }
        }
        self.last_nick.clone()
    }

    /// Main update method called from JavaScript animation frame
    pub fn update(&mut self) -> Result<(), JsValue> {
        let now = utils::now();
        let frame_dt = (((now - self.last_update) / 1000.0).max(0.0).min(FRAME_DT_MAX as f64)) as f32;
        self.last_update = now;
        
        // Process WebSocket event flags
        if self.ws_open_flag.get() {
            self.ws_open_flag.set(false);
            self.handle_ws_open();
        }
        
        if self.ws_close_flag.get() {
            self.ws_close_flag.set(false);
            self.handle_disconnect();
        }
        
        // Process key press events (only send on initial press, not while held)
        let (should_split, should_eject, should_q, should_e, should_r, should_t, should_p, should_enter, should_escape) = {
            let mut input = self.input_state.borrow_mut();
            
            let should_split = input.space_just_pressed();
            let should_eject = input.w_just_pressed();
            let should_q = input.q_just_pressed();
            let should_e = input.e_just_pressed();
            let should_r = input.r_just_pressed();
            let should_t = input.t_just_pressed();
            let should_p = input.p_just_pressed();
            let should_enter = input.enter_just_pressed();
            let should_escape = input.escape_just_pressed();
            
            // Update previous frame state for next frame's edge detection
            input.update_previous_state();
            
            (should_split, should_eject, should_q, should_e, should_r, should_t, should_p, should_enter, should_escape)
        };
        
        // Check WebSocket state once for all actions
        let ws_open = {
            let conn = self.connection.borrow();
            conn.websocket().ready_state() == 1  // OPEN state
        };
        
        // Now process the actions without any borrows held (only if WebSocket is open)
        if ws_open {
            if should_split {
                if let Err(e) = self.connection.borrow().send_split() {
                    web_sys::console::error_1(&format!("Failed to send split: {:?}", e).into());
                }
            }
            
            if should_eject {
                if let Err(e) = self.connection.borrow().send_eject() {
                    web_sys::console::error_1(&format!("Failed to send eject: {:?}", e).into());
                }
            }
            
            if should_q {
                // Q is for freezing (server-side feature)
                if let Err(e) = self.connection.borrow().send_q() {
                    web_sys::console::error_1(&format!("Failed to send Q: {:?}", e).into());
                }
            }
            
            if should_e {
                if let Err(e) = self.connection.borrow().send_e() {
                    web_sys::console::error_1(&format!("Failed to send E: {:?}", e).into());
                }
            }
            
            if should_r {
                if let Err(e) = self.connection.borrow().send_r() {
                    web_sys::console::error_1(&format!("Failed to send R: {:?}", e).into());
                }
            }
            
            if should_t {
                if let Err(e) = self.connection.borrow().send_t() {
                    web_sys::console::error_1(&format!("Failed to send T: {:?}", e).into());
                }
            }

            if should_p {
                if let Err(e) = self.connection.borrow().send_p() {
                    web_sys::console::error_1(&format!("Failed to send P: {:?}", e).into());
                }
            }
        }
        
        if should_enter {
            // Enter key - focus chat input
            self.ui.focus_chat_input();
        }
        
        if should_escape {
            // Escape key - could be used to close UI or enter spectate mode
            if let Err(e) = self.connection.borrow().send_spectate() {
                web_sys::console::error_1(&format!("Failed to send spectate: {:?}", e).into());
            }
        }

        // FPS tracking — update stats display once per second
        self.frame_count += 1;
        if now - self.last_fps_time >= 1000.0 {
            self.fps = self.frame_count;
            self.frame_count = 0;
            self.last_fps_time = now;
            let score = self.calculate_score();
            self.ui.update_stats(self.fps, score, self.my_cells.len());
        }

        // Send stats request every 2 seconds (matches JS implementation)
        if ws_open && now - self.last_stats_request >= 2000.0 {
            self.last_stats_request = now;
            if let Err(e) = self.connection.borrow().send_stats_request() {
                web_sys::console::error_1(&format!("Failed to send stats request: {:?}", e).into());
            }
        }

        // Process pending spawn request
        let spawn_nick = self.pending_spawn.borrow_mut().take();
        if let Some(nick) = spawn_nick {
            self.spawn(&nick);
        }

        // Process all queued packets from WebSocket
        let packets: Vec<Vec<u8>> = self.packet_queue.borrow_mut().drain(..).collect();
        for packet_data in packets {
            self.handle_packet(packet_data);
        }

        // Check for death overlay delay (250ms after death)
        if let Some(death_time) = self.death_time {
            if !self.alive && self.my_cells.is_empty() && now - death_time >= 250.0 {
                self.ui.show_login_overlay(&self.last_nick, self.last_skin.as_deref());
                self.death_time = None; // Clear so we don't show repeatedly
            }
        }

        // Clean up destroyed cells that have finished their fade-out animation
        let cells_to_remove: Vec<u32> = self.cells.iter()
            .filter_map(|(id, cell)| {
                if cell.should_remove() {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        
        for cell_id in cells_to_remove {
            self.cells.remove(&cell_id);
        }

        // Read input state and update mouse world position
        let (mouse_pos, space, w, q) = {
            let input = self.input_state.borrow();
            (input.mouse_pos, input.space_pressed, input.w_pressed, input.q_pressed)
        };

        let screen_center = Vec2::new(
            self.renderer.width() / 2.0,
            self.renderer.height() / 2.0
        );
        self.mouse_world_pos = self.camera.screen_to_world(
            mouse_pos,
            screen_center
        );

        // Interpolate all cells (JS behavior): dt = clamp((now - updated) / 120, 0, 1)
        // First pass: collect killer positions for destroyed cells
        let killer_positions: std::collections::HashMap<u32, Vec2> = self.cells.iter()
            .filter_map(|(id, cell)| {
                if Self::is_potential_killer(cell) {
                    Some((*id, cell.position))
                } else {
                    None
                }
            })
            .collect();

        let killer_is_mother: std::collections::HashMap<u32, bool> = self.cells.iter()
            .filter_map(|(id, cell)| {
                if Self::is_potential_killer(cell) {
                    Some((*id, Self::is_mother_cell(cell)))
                } else {
                    None
                }
            })
            .collect();
        
        // Collect cell data needed for distance calculations (avoid full clone)
        let cell_sizes: std::collections::HashMap<u32, f32> = self.cells.iter()
            .map(|(id, cell)| (*id, cell.render_size))
            .collect();
        
        for cell in self.cells.values_mut() {
            // If cell is destroyed and has a killer, move toward the killer
            if cell.is_destroyed && cell.killed_by.is_some() {
                if let Some(killer_id) = cell.killed_by {
                    if let Some(killer_pos) = killer_positions.get(&killer_id) {
                        // Found the specific killer, move toward it
                        cell.target_position = *killer_pos;
                    } else {
                        // Killer not visible, find nearest viable cell as fallback
                        if let Some(nearest_pos) = Self::find_nearest_viable_target_optimized(
                            &cell.position,
                            cell.render_size,
                            &killer_positions,
                            &cell_sizes,
                            &killer_is_mother,
                        ) {
                            cell.target_position = nearest_pos;
                        }
                    }
                }
            }
            
            let dt = (((now - cell.update_time) / INTERPOLATION_DURATION_MS).max(0.0).min(1.0)) as f32;
            cell.position.x = cell.ox + (cell.target_position.x - cell.ox) * dt;
            cell.position.y = cell.oy + (cell.target_position.y - cell.oy) * dt;
            cell.size        = cell.os + (cell.target_size        - cell.os) * dt;

            // Render uses the same interpolated state to match server timing.
            cell.render_position = cell.position;
            cell.render_size = cell.size;
        }

        // Update camera to follow player cells (uses interpolated positions/sizes)
        let has_cells = !self.my_cells.is_empty();
        if has_cells {
            let positions: Vec<Vec2> = self.my_cells.iter()
                .filter_map(|&id| self.cells.get(&id).map(|c| c.render_position))
                .collect();
            let sizes: Vec<f32> = self.my_cells.iter()
                .filter_map(|&id| self.cells.get(&id).map(|c| c.render_size))
                .collect();

            if !positions.is_empty() {
                self.camera.follow_cells(&positions, &sizes);
            }
        }

        self.camera.update(has_cells);

        // Jelly physics with LOD (skips small cells)
        if self.settings.jelly_physics {
            self.update_jelly_physics();
        }

        // Send mouse position to server (throttled to ~25 times/sec)
        // Only send if WebSocket is open
        if now - self.last_mouse_send > MOUSE_SEND_INTERVAL_MS {
            let ws_open = self.connection.borrow().websocket().ready_state() == 1;  // OPEN state
            if ws_open {
                if let Err(e) = self.connection.borrow().send_mouse(
                    self.mouse_world_pos.x,
                    self.mouse_world_pos.y
                ) {
                    web_sys::console::error_1(&format!("Failed to send mouse: {:?}", e).into());
                }
                self.last_mouse_send = now;
            }
        }

        // Render
        self.render()?;

        Ok(())
    }

    fn render(&self) -> Result<(), JsValue> {
        let background = if self.settings.dark_theme { "#111" } else { "#f2f2f2" };
        self.renderer.clear(background);
        if self.settings.show_grid {
            self.renderer.draw_grid(self.border, self.camera.position, self.camera.zoom, self.settings.dark_theme);
        }
        if self.settings.show_background_sectors {
            self.renderer.draw_background_sectors(
                self.border,
                self.camera.position,
                self.camera.zoom,
                self.settings.dark_theme,
            );
        }
        self.renderer.draw_border(self.border, self.camera.position, self.camera.zoom);

        // Calculate viewport bounds for culling
        let screen_center = Vec2::new(self.renderer.width() / 2.0, self.renderer.height() / 2.0);
        let half_view_w = screen_center.x / self.camera.zoom;
        let half_view_h = screen_center.y / self.camera.zoom;
        let view_min_x = self.camera.position.x - half_view_w - 100.0; // Extra margin for large cells
        let view_max_x = self.camera.position.x + half_view_w + 100.0;
        let view_min_y = self.camera.position.y - half_view_h - 100.0;
        let view_max_y = self.camera.position.y + half_view_h + 100.0;

        // Sort cells by size (draw larger cells first, smaller on top)
        // Only include cells that are potentially visible
        let mut cells_to_draw: Vec<&Cell> = self.cells.values()
            .filter(|cell| {
                let pos = cell.render_position;
                let size = cell.render_size;
                // Quick AABB check - cell is visible if it overlaps viewport
                pos.x + size >= view_min_x && pos.x - size <= view_max_x &&
                pos.y + size >= view_min_y && pos.y - size <= view_max_y
            })
            .collect();
        cells_to_draw.sort_by(|a, b| {
            match a.render_size.partial_cmp(&b.render_size) {
                Some(std::cmp::Ordering::Equal) => a.id.cmp(&b.id),
                Some(order) => order,
                None => std::cmp::Ordering::Equal,
            }
        });

        for cell in cells_to_draw {
            let skin_img = if self.settings.show_skins {
                cell.skin.as_ref().and_then(|s| self.skins.get(s))
            } else {
                None
            };
            let alpha = cell.get_render_alpha();
            if alpha > 0.0 {
                self.renderer.draw_cell(
                    cell,
                    self.camera.position,
                    self.camera.zoom,
                    skin_img,
                    self.settings.show_names,
                    self.settings.show_mass,
                    self.settings.jelly_physics,
                    alpha,
                );
            }
        }

        // Minimap — visible once the player has spawned at least once
        if self.settings.show_minimap && !self.last_nick.is_empty() {
            let my_cell_data: Vec<(Vec2, f32, (u8, u8, u8))> = self.my_cells.iter()
                .filter_map(|&id| self.cells.get(&id).map(|c| (c.render_position, c.render_size, c.color)))
                .collect();
            let xray_recent = (utils::now() - self.xray_last_update) <= 5000.0;
            let xray_points: Vec<(u32, Vec2, f32, (u8, u8, u8), String)> = if xray_recent {
                self.xray_players
                    .iter()
                    .map(|p| (p.id, p.position, p.size, p.color, p.name.clone()))
                    .collect()
            } else {
                Vec::new()
            };
            self.minimap.draw(
                self.border,
                &my_cell_data,
                self.camera.position,
                self.camera.zoom,
                self.renderer.width(),
                self.renderer.height(),
                self.settings.dark_theme,
                &xray_points,
            );
        }

        Ok(())
    }

    fn calculate_score(&self) -> f32 {
        self.my_cells.iter()
            .filter_map(|id| self.cells.get(id))
            .map(|c| (c.target_size * c.target_size / 100.0).floor())
            .sum()
    }

    pub fn handle_mouse_move(&mut self, screen_x: f32, screen_y: f32) {
        let screen_center = Vec2::new(
            self.renderer.width() / 2.0,
            self.renderer.height() / 2.0
        );
        self.mouse_world_pos = self.camera.screen_to_world(
            Vec2::new(screen_x, screen_y),
            screen_center
        );
    }

    pub fn handle_key_down(&mut self, key: &str) {
        match key {
            " " => {
                self.input_state.borrow_mut().space_pressed = true;
            }
            "w" | "W" => {
                self.input_state.borrow_mut().w_pressed = true;
            }
            "q" | "Q" => {
                self.input_state.borrow_mut().q_pressed = true;
            }
            "e" | "E" => {
                self.input_state.borrow_mut().e_pressed = true;
            }
            "r" | "R" => {
                self.input_state.borrow_mut().r_pressed = true;
            }
            "t" | "T" => {
                self.input_state.borrow_mut().t_pressed = true;
            }
            "p" | "P" => {
                self.input_state.borrow_mut().p_pressed = true;
            }
            "Enter" => {
                self.input_state.borrow_mut().enter_pressed = true;
            }
            "Escape" => {
                self.input_state.borrow_mut().escape_pressed = true;
            }
            _ => {}
        }
    }
    
    pub fn handle_key_up(&mut self, key: &str) {
        // Update input state when keys are released
        match key {
            " " => {
                self.input_state.borrow_mut().space_pressed = false;
            }
            "w" | "W" => {
                self.input_state.borrow_mut().w_pressed = false;
            }
            "q" | "Q" => {
                self.input_state.borrow_mut().q_pressed = false;
            }
            "e" | "E" => {
                self.input_state.borrow_mut().e_pressed = false;
            }
            "r" | "R" => {
                self.input_state.borrow_mut().r_pressed = false;
            }
            "t" | "T" => {
                self.input_state.borrow_mut().t_pressed = false;
            }
            "p" | "P" => {
                self.input_state.borrow_mut().p_pressed = false;
            }
            "Enter" => {
                self.input_state.borrow_mut().enter_pressed = false;
            }
            "Escape" => {
                self.input_state.borrow_mut().escape_pressed = false;
            }
            _ => {}
        }
    }

    // Handle incoming packet
    pub fn handle_packet(&mut self, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }

        let mut reader = BinaryReader::new(data);
        if let Err(e) = self.try_handle_packet(&mut reader) {
            web_sys::console::error_1(&format!("Packet parsing error: {:?}", e).into());
        }
    }

    fn try_handle_packet(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        let opcode = match reader.try_get_u8() {
            Some(op) => op,
            None => return Err("Empty packet".to_string()),
        };

        match opcode {
            0x10 => self.handle_update_nodes(reader),   // World update
            0x11 => self.handle_update_position(reader), // Spectator position
            0x12 => self.handle_clear_all(reader),       // Clear all cells
            0x14 => self.handle_clear_owned(reader),     // Clear my cells
            0x15 => self.handle_draw_line(reader),       // Draw line (experimental)
            0x20 => self.handle_add_node(reader),        // Add my cell
            0x31 => self.handle_leaderboard_ffa(reader), // FFA leaderboard
            0x32 => self.handle_leaderboard_teams(reader), // Teams leaderboard
            0x40 => self.handle_set_border(reader),      // Set border
            0x50 => self.handle_xray_data(reader),       // Xray data
            0x63 => self.handle_chat(reader),            // Chat message
            0xFE => self.handle_server_stat(reader),     // Server stats
            _ => {
                web_sys::console::warn_1(&format!("Unknown opcode: 0x{:02X}", opcode).into());
                Ok(())
            }
        }
    }

    fn handle_clear_all(&mut self, _reader: &mut BinaryReader) -> Result<(), String> {
        let had_cells = !self.my_cells.is_empty();
        self.cells.clear();
        self.my_cells.clear();
        self.alive = false;
        if had_cells {
            self.death_time = Some(utils::now());
        }
        Ok(())
    }

    fn handle_clear_owned(&mut self, _reader: &mut BinaryReader) -> Result<(), String> {
        let had_cells = !self.my_cells.is_empty();
        self.my_cells.clear();
        self.alive = false;
        if had_cells {
            self.death_time = Some(utils::now());
        }
        Ok(())
    }

    fn handle_add_node(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // Node ID is already XOR'd with scramble_id on the wire — use as-is.
        // All packets use the same scramble_id, so IDs match consistently.
        let node_id = reader.try_get_u32().ok_or("truncated add_node packet")?;
        if !self.my_cells.contains(&node_id) {
            self.my_cells.push(node_id);
        }
        self.alive = true;
        self.death_time = None;
        Ok(())
    }

    fn handle_set_border(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // Border coordinates already include scramble (server adds scramble_x/y).
        // Store as-is — all cell coords are in the same scrambled space.
        let min_x = reader.try_get_f64().ok_or("truncated border packet")? as f32;
        let min_y = reader.try_get_f64().ok_or("truncated border packet")? as f32;
        let max_x = reader.try_get_f64().ok_or("truncated border packet")? as f32;
        let max_y = reader.try_get_f64().ok_or("truncated border packet")? as f32;

        self.border = (min_x, min_y, max_x, max_y);

        // Center camera on map when border is first received (for spectator view)
        if !self.alive && self.my_cells.is_empty() {
            let center_x = (min_x + max_x) / 2.0;
            let center_y = (min_y + max_y) / 2.0;
            self.camera.position = Vec2::new(center_x, center_y);
            self.camera.target_position = Vec2::new(center_x, center_y);
        }

        // Optional trailing: game_type (u32) + server_name (utf8 string)
        if reader.remaining() >= 4 {
            let _game_type = reader.try_get_u32();
            let _server_name = reader.get_string_utf8();
        }

        Ok(())
    }

    fn handle_xray_data(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        let player_count = reader.try_get_u16().ok_or("truncated xray count")?;
        let mut players = Vec::with_capacity(player_count as usize);

        for _ in 0..player_count {
            let id = reader.try_get_u32().ok_or("truncated xray id")?;
            let x_raw = reader.try_get_u32().ok_or("truncated xray x")?;
            let y_raw = reader.try_get_u32().ok_or("truncated xray y")?;

            let x = if x_raw > i32::MAX as u32 {
                (x_raw as i64 - 4294967296) as i32
            } else {
                x_raw as i32
            } as f32;
            let y = if y_raw > i32::MAX as u32 {
                (y_raw as i64 - 4294967296) as i32
            } else {
                y_raw as i32
            } as f32;

            let size = reader.try_get_u16().ok_or("truncated xray size")? as f32;
            let r = reader.try_get_u8().ok_or("truncated xray color r")?;
            let g = reader.try_get_u8().ok_or("truncated xray color g")?;
            let b = reader.try_get_u8().ok_or("truncated xray color b")?;
            let name = reader.get_string_utf8();

            players.push(XrayPlayer {
                id,
                position: Vec2::new(x, y),
                size,
                color: (r, g, b),
                name,
            });
        }

        self.xray_players = players;
        self.xray_last_update = utils::now();
        Ok(())
    }

    /// Parse 0x10 UpdateNodes packet.
    ///
    /// Wire format (protocol >= 11, matching write_update_nodes_v11 in server):
    ///   u16  eat_count
    ///   [u32 eater_id, u32 eaten_id] × eat_count
    ///   loop (updates then adds, no distinction on wire):
    ///     u32  node_id          — 0 terminates the loop
    ///     i32  x
    ///     i32  y
    ///     u16  size
    ///     u8   flags
    ///     u8   extended        — only if flags & 0x80 (is_food)
    ///     [u8 r, u8 g, u8 b]  — only if flags & 0x02 (is_player / has color)
    ///     string_utf8 skin     — only if flags & 0x04 (has_skin); protocol 11+ prefixes with '%'
    ///     string_utf8 name     — only if flags & 0x08 (has_name)
    ///   u16  remove_count
    ///   [u32 node_id] × remove_count
    ///
    /// Flag bits (CellFlags::encode_v6 / v11):
    ///   0x01 is_spiked  (virus)
    ///   0x02 is_player  (color present)
    ///   0x04 has_skin
    ///   0x08 has_name
    ///   0x10 is_agitated
    ///   0x20 is_ejected
    ///   0x80 is_food
    fn handle_update_nodes(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // --- Eat events ---
        let eat_count = reader.try_get_u16().ok_or("truncated eat_count")?;
        if eat_count > 0 {
            self.saw_eat_record = true;
        }
        for _ in 0..eat_count {
            let eater_id = reader.try_get_u32().ok_or("truncated eat eater_id")?;
            let eaten_id  = reader.try_get_u32().ok_or("truncated eat eaten_id")?;

            // Mark the eaten cell as destroyed for animation, don't remove immediately
            let eater_pos = self.cells.get(&eater_id).map(|c| c.position);
            if let Some(cell) = self.cells.get_mut(&eaten_id) {
                cell.destroy(Some(eater_id));
                if let Some(pos) = eater_pos {
                    // Seed target position so short-lived food/ejected anims are visible
                    let now = utils::now();
                    let dt = (((now - cell.update_time) / 120.0).max(0.0).min(1.0)) as f32;
                    cell.position.x = cell.ox + (cell.target_position.x - cell.ox) * dt;
                    cell.position.y = cell.oy + (cell.target_position.y - cell.oy) * dt;
                    cell.size        = cell.os + (cell.target_size        - cell.os) * dt;
                    cell.ox = cell.position.x;
                    cell.oy = cell.position.y;
                    cell.os = cell.size;
                    cell.target_position = pos;
                    cell.update_time = now;
                }
            }
            
            // Remove from my_cells list immediately if it's mine
            self.my_cells.retain(|&id| id != eaten_id);
        }
        
        // Check if player died (all cells eaten)
        if self.my_cells.is_empty() && self.alive {
            self.alive = false;
            self.death_time = Some(utils::now());
        }

        // --- Node updates + adds (terminated by node_id == 0) ---
        loop {
            let node_id = reader.try_get_u32().ok_or("truncated node_id")?;
            if node_id == 0 {
                break;
            }

            let x    = reader.try_get_i32().ok_or("truncated x")?    as f32;
            let y    = reader.try_get_i32().ok_or("truncated y")?    as f32;
            let size = reader.try_get_u16().ok_or("truncated size")? as f32;
            let flags = reader.try_get_u8().ok_or("truncated flags")?;

            // Color — present when is_player flag is set (server always sets this)
            let (r, g, b) = if flags & 0x02 != 0 {
                let r = reader.try_get_u8().ok_or("truncated color r")?;
                let g = reader.try_get_u8().ok_or("truncated color g")?;
                let b = reader.try_get_u8().ok_or("truncated color b")?;
                (r, g, b)
            } else {
                (200, 200, 200)
            };

            // Skin — only on initial add (has_skin flag set)
            let skin = if flags & 0x04 != 0 {
                let s = reader.get_string_utf8();
                Self::normalize_skin_name(&s)
            } else {
                None
            };

            // Kick off image fetch for any new skin we haven't seen yet
            if let Some(ref skin_name) = skin {
                self.ensure_skin_loaded(skin_name);
            }

            // Name — only on initial add (has_name flag set)
            let name = if flags & 0x08 != 0 {
                reader.get_string_utf8()
            } else {
                String::new()
            };

            let is_virus   = (flags & 0x01) != 0;
            let is_ejected = (flags & 0x20) != 0;
            let is_food    = (flags & 0x80) != 0;

            // Coordinates are already in scrambled space (server added scramble_x/y).
            // Store directly — border is in the same space, camera operates here too.
            let is_mine = self.my_cells.contains(&node_id);
            if let Some(cell) = self.cells.get_mut(&node_id) {
                // Snap interpolation to current time before resetting lerp (matches JS cell.update() call)
                let now = utils::now();
                let dt = (((now - cell.update_time) / 120.0).max(0.0).min(1.0)) as f32;
                cell.position.x = cell.ox + (cell.target_position.x - cell.ox) * dt;
                cell.position.y = cell.oy + (cell.target_position.y - cell.oy) * dt;
                cell.size        = cell.os + (cell.target_size        - cell.os) * dt;

                // Current interpolated pos/size become the new lerp start
                cell.ox = cell.position.x;
                cell.oy = cell.position.y;
                cell.os = cell.size;
                cell.target_position = Vec2::new(x, y);
                cell.target_size     = size;
                cell.update_time     = now;

                cell.color = (r, g, b);
                if !name.is_empty()  { cell.name = name; }
                if skin.is_some()    { cell.skin = skin; }
                cell.is_virus   = is_virus;
                cell.is_ejected = is_ejected;
                cell.is_food    = is_food;
            } else {
                let mut cell = Cell::new(node_id, x, y, size, (r, g, b));
                cell.name        = name;
                cell.skin        = skin;
                cell.is_virus    = is_virus;
                cell.is_ejected  = is_ejected;
                cell.is_food     = is_food;
                self.cells.insert(node_id, cell);
            }
        }

        // --- Removed nodes ---
        let remove_count = reader.try_get_u16().ok_or("truncated remove_count")?;
        for _ in 0..remove_count {
            let node_id = reader.try_get_u32().ok_or("truncated remove node_id")?;
            
            // Find nearest viable target before getting mutable reference
            let nearest_id = if self.saw_eat_record {
                None
            } else if let Some(target_cell) = self.cells.get(&node_id) {
                if target_cell.is_destroyed && target_cell.killed_by.is_some() {
                    // Eat record already told us the killer; don't override it.
                    target_cell.killed_by
                } else {
                    let available_cells: std::collections::HashMap<u32, Vec2> = self.cells.iter()
                        .filter(|(id, other_cell)| **id != node_id && Self::is_potential_killer(other_cell))
                        .map(|(id, cell)| (*id, cell.position))
                        .collect();
                    
                    Self::find_nearest_viable_target(&target_cell.position, target_cell.render_size, &available_cells, &self.cells)
                        .and_then(|target_pos| {
                            // Find which cell ID corresponds to this position
                            self.cells.iter()
                                .find(|(id, cell)| **id != node_id && !cell.is_destroyed && cell.position == target_pos)
                                .map(|(id, _)| *id)
                        })
                }
            } else {
                None
            };
            
            // Mark the cell as destroyed for animation, don't remove immediately
            if let Some(cell) = self.cells.get_mut(&node_id) {
                if cell.is_destroyed {
                    if cell.killed_by.is_none() {
                        cell.killed_by = nearest_id;
                    }
                } else {
                    cell.destroy(nearest_id);
                }
            }
            
            // Remove from my_cells list immediately if it's mine
            self.my_cells.retain(|&id| id != node_id);
        }
        
        // Check if player died (all cells removed)
        if self.my_cells.is_empty() && self.alive {
            self.alive = false;
            self.death_time = Some(utils::now());
        }

        Ok(())
    }

    fn handle_update_position(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // Spectator position update — use to drive camera when not alive
        let x    = reader.try_get_f32().ok_or("truncated spectator x")?;
        let y    = reader.try_get_f32().ok_or("truncated spectator y")?;
        let zoom = reader.try_get_f32().ok_or("truncated spectator zoom")?;
        if !self.alive {
            if self.camera.position == Vec2::ZERO && self.camera.target_position == Vec2::ZERO {
                self.camera.position = Vec2::new(x, y);
                self.camera.zoom = zoom * self.camera.zoom_factor;
            }
            self.camera.target_position = Vec2::new(x, y);
            self.camera.set_base_zoom(zoom);
        }
        Ok(())
    }

    fn handle_draw_line(&mut self, _reader: &mut BinaryReader) -> Result<(), String> {
        // Experimental mode feature — not implemented
        Ok(())
    }

    /// Parse 0x31 LeaderboardFFA.
    /// Format: u32 count, then [u32 is_me, string_utf8 name] × count
    fn handle_leaderboard_ffa(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        let count = reader.try_get_u32().ok_or("truncated leaderboard count")?;
        self.leaderboard.clear();
        for _ in 0..count {
            let is_me = reader.try_get_u32().unwrap_or(0) != 0;
            let name  = reader.get_string_utf8();
            self.leaderboard.push((is_me, name));
        }
        self.ui.update_leaderboard(&self.leaderboard);
        Ok(())
    }

    fn handle_leaderboard_teams(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // Pie-chart leaderboard: u32 count, then [f32 team_size] × count
        let count = reader.try_get_u32().unwrap_or(0);
        for _ in 0..count {
            reader.try_get_f32(); // consume
        }
        Ok(())
    }

    fn handle_server_stat(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        // Parse server statistics JSON
        let json_str = reader.get_string_utf8();
        
        match serde_json::from_str::<ServerStats>(&json_str) {
            Ok(stats) => {
                // Calculate latency
                let now = utils::now();
                self.latency = Some(now - self.last_stats_request);
                
                // Store stats and update UI
                self.server_stats = Some(stats.clone());
                self.ui.update_server_stats(&stats, self.latency);
            }
            Err(e) => {
                web_sys::console::warn_1(&format!("Failed to parse server stats: {:?}", e).into());
            }
        }
        Ok(())
    }

    /// Parse 0x63 ChatMessage.
    /// Format: u8 flags, u8 r, u8 g, u8 b, string_utf8 name, string_utf8 message
    fn handle_chat(&mut self, reader: &mut BinaryReader) -> Result<(), String> {
        let _flags = reader.try_get_u8().ok_or("truncated chat flags")?;
        let r = reader.try_get_u8().unwrap_or(255);
        let g = reader.try_get_u8().unwrap_or(255);
        let b = reader.try_get_u8().unwrap_or(255);
        let name    = reader.get_string_utf8();
        let message = reader.get_string_utf8();

        self.ui.show_chat_message(&name, &message, (r, g, b));
        Ok(())
    }

    fn update_jelly_physics(&mut self) {
        const QUADTREE_MAX_POINTS: usize = 32;
        const CELL_POINTS_MIN: usize = 5;
        const CELL_POINTS_MAX: usize = 120;
        const VIRUS_POINTS: usize = 100;

        let size_scale = self.camera.size_scale.max(0.001);
        let w = 1920.0 / size_scale;
        let h = 1080.0 / size_scale;
        let x = self.camera.position.x - w / 2.0;
        let y = self.camera.position.y - h / 2.0;

        let mut quadtree = PointQuadTree::new(x, y, w, h, QUADTREE_MAX_POINTS as f32);

        // Build quadtree from existing points (JS behavior).
        for cell in self.cells.values() {
            if cell.points.is_empty() {
                continue;
            }
            for point in &cell.points {
                quadtree.insert(PointRef {
                    x: point.x,
                    y: point.y,
                    parent_id: cell.id,
                });
            }
        }

        let (min_x, min_y, max_x, max_y) = self.border;

        for cell in self.cells.values_mut() {
            // LOD: Skip jelly physics for small cells (< 25px screen radius)
            let screen_radius = cell.render_size * self.camera.zoom;
            if screen_radius < 25.0 {
                // Clear jelly points for small cells to save memory
                if !cell.points.is_empty() {
                    cell.points.clear();
                    cell.points_vel.clear();
                }
                continue;
            }

            let mut num_points = screen_radius.floor() as i32;
            num_points = num_points.clamp(CELL_POINTS_MIN as i32, CELL_POINTS_MAX as i32);
            let mut num_points = num_points as usize;
            if cell.is_virus {
                num_points = VIRUS_POINTS;
            }

            while cell.points.len() > num_points {
                let idx = (Math::random() * cell.points.len() as f64) as usize;
                cell.points.remove(idx);
                cell.points_vel.remove(idx);
            }

            if cell.points.is_empty() && num_points > 0 {
                cell.points.push(RenderPoint {
                    x: cell.render_position.x,
                    y: cell.render_position.y,
                    rl: cell.render_size,
                });
                cell.points_vel.push(Math::random() as f32 - 0.5);
            }

            while cell.points.len() < num_points {
                let idx = (Math::random() * cell.points.len() as f64) as usize;
                let point = cell.points[idx];
                let vel = cell.points_vel[idx];
                cell.points.insert(idx, point);
                cell.points_vel.insert(idx, vel);
            }

            let len = cell.points.len();
            if len == 0 {
                continue;
            }

            let prev_vels = cell.points_vel.clone();
            for i in 0..len {
                let prev = prev_vels[(i + len - 1) % len];
                let next = prev_vels[(i + 1) % len];
                let new_vel = ((cell.points_vel[i] + Math::random() as f32 - 0.5) * 0.7)
                    .clamp(-10.0, 10.0);
                cell.points_vel[i] = (prev + next + 8.0 * new_vel) / 10.0;
            }

            for i in 0..len {
                let prev_rl = cell.points[(i + len - 1) % len].rl;
                let next_rl = cell.points[(i + 1) % len].rl;
                let mut cur_rl = cell.points[i].rl;

                let mut affected = quadtree.some(
                    Aabb::new(cell.points[i].x - 5.0, cell.points[i].y - 5.0, 10.0, 10.0),
                    |p| p.parent_id != cell.id && sq_dist(p.x, p.y, cell.points[i].x, cell.points[i].y) <= 25.0,
                );

                if !affected && (cell.points[i].x < min_x || cell.points[i].y < min_y || cell.points[i].x > max_x || cell.points[i].y > max_y) {
                    affected = true;
                }

                if affected {
                    cell.points_vel[i] = cell.points_vel[i].min(0.0) - 1.0;
                }

                cur_rl += cell.points_vel[i];
                cur_rl = cur_rl.max(0.0);
                cur_rl = (9.0 * cur_rl + cell.render_size) / 10.0;
                cell.points[i].rl = (prev_rl + next_rl + 8.0 * cur_rl) / 10.0;

                let angle = 2.0 * std::f32::consts::PI * i as f32 / len as f32;
                let mut rl = cell.points[i].rl;
                if cell.is_virus && i % 2 == 0 {
                    rl += 5.0;
                }
                cell.points[i].x = cell.render_position.x + angle.cos() * rl;
                cell.points[i].y = cell.render_position.y + angle.sin() * rl;
            }
        }
    }
}

fn sq_dist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

#[derive(Clone, Copy)]
struct Aabb {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Aabb {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn contains(&self, point: PointRef) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }
}

struct QuadNode {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    points: Vec<PointRef>,
    children: Option<Vec<QuadNode>>,
}

impl QuadNode {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            points: Vec::new(),
            children: None,
        }
    }

    fn contains_point(&self, point: PointRef) -> bool {
        point.x >= self.x && point.x <= self.x + self.w && point.y >= self.y && point.y <= self.y + self.h
    }

    fn overlaps(&self, aabb: Aabb) -> bool {
        aabb.x < self.x + self.w
            && aabb.x + aabb.w > self.x
            && aabb.y < self.y + self.h
            && aabb.y + aabb.h > self.y
    }

    fn insert(&mut self, point: PointRef, max_points: f32) {
        if let Some(children) = self.children.as_mut() {
            let col = (point.x > self.x + self.w / 2.0) as usize;
            let row = (point.y > self.y + self.h / 2.0) as usize;
            let idx = col + row * 2;
            children[idx].insert(point, max_points * 1.1);
            return;
        }

        self.points.push(point);
        if self.points.len() as f32 > max_points && self.w > 1.0 {
            self.split(max_points);
        }
    }

    fn split(&mut self, max_points: f32) {
        let half_w = self.w / 2.0;
        let half_h = self.h / 2.0;
        let mut children = Vec::with_capacity(4);
        for row in 0..2 {
            for col in 0..2 {
                let px = self.x + col as f32 * half_w;
                let py = self.y + row as f32 * half_h;
                children.push(QuadNode::new(px, py, half_w, half_h));
            }
        }

        let old_points = std::mem::take(&mut self.points);
        let mid_x = self.x + half_w;
        let mid_y = self.y + half_h;
        for point in old_points {
            let col = (point.x > mid_x) as usize;
            let row = (point.y > mid_y) as usize;
            let idx = col + row * 2;
            children[idx].insert(point, max_points * 1.1);
        }
        self.children = Some(children);
    }

    fn some<F>(&self, aabb: Aabb, test: &F) -> bool
    where
        F: Fn(PointRef) -> bool,
    {
        if let Some(children) = &self.children {
            for child in children {
                if child.overlaps(aabb) && child.some(aabb, test) {
                    return true;
                }
            }
        } else {
            for &point in &self.points {
                if aabb.contains(point) && test(point) {
                    return true;
                }
            }
        }
        false
    }
}

struct PointQuadTree {
    root: QuadNode,
    max_points: f32,
}

impl PointQuadTree {
    fn new(x: f32, y: f32, w: f32, h: f32, max_points: f32) -> Self {
        Self {
            root: QuadNode::new(x, y, w, h),
            max_points,
        }
    }

    fn insert(&mut self, point: PointRef) {
        if !self.root.contains_point(point) {
            return;
        }
        self.root.insert(point, self.max_points);
    }

    fn some<F>(&self, aabb: Aabb, test: F) -> bool
    where
        F: Fn(PointRef) -> bool,
    {
        self.root.some(aabb, &test)
    }
}

