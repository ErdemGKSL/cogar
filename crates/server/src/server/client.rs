//! Client session state.

use protocol::Color;
use std::collections::HashSet;
use std::net::SocketAddr;

/// A connected client session.
#[derive(Debug)]
pub struct Client {
    /// Unique client ID.
    pub id: u32,
    /// Remote address.
    pub addr: SocketAddr,
    /// Protocol version (set during handshake).
    pub protocol: u32,
    /// Whether handshake is complete.
    pub handshake_complete: bool,
    /// Player name.
    pub name: String,
    /// Skin name.
    pub skin: Option<String>,
    /// Player color.
    pub color: Color,
    /// Mouse position.
    pub mouse_x: i32,
    pub mouse_y: i32,
    /// Cell IDs owned by this player.
    pub cells: Vec<u32>,
    /// Scramble values for anti-cheat.
    pub scramble_id: u32,
    pub scramble_x: i32,
    pub scramble_y: i32,
    /// Is spectating.
    pub is_spectating: bool,
    /// Is operator.
    pub is_operator: bool,
    /// Last activity timestamp.
    pub last_activity: std::time::Instant,

    // Viewport state
    /// Center position (average of owned cells, or spectate position).
    pub center_x: f32,
    pub center_y: f32,
    /// Current zoom scale.
    pub scale: f32,
    /// Viewport bounds.
    pub view_min_x: f32,
    pub view_min_y: f32,
    pub view_max_x: f32,
    pub view_max_y: f32,

    /// Cells currently visible to this client (what they've been sent).
    pub client_nodes: HashSet<u32>,
    /// Cells in the current view (to be sent).
    pub view_nodes: Vec<u32>,

    /// Whether this client needs a border packet.
    pub needs_border: bool,
    /// Number of ticks since last leaderboard update.
    pub leaderboard_tick: u32,
    /// Last tick when player ejected mass (for cooldown).
    pub last_eject_tick: u64,
    /// Last tick when ServerStat was sent to this client (rate-limit).
    pub last_stat_tick: u64,
    /// Player team (0=Red, 1=Green, 2=Blue).
    pub team: Option<u8>,

    // Minion control flags
    /// Whether minion control mode is active.
    pub minion_control: bool,
    /// Minion IDs owned by this player.
    pub minions: Vec<u32>,
    /// Latest minion number assigned (for naming).
    pub latest_minion_id: u16,
    /// Minion follow mode: follow owner center (true) vs mouse (false).
    pub minion_follow: bool,
    /// One-shot: trigger minion split this tick.
    pub minion_split: bool,
    /// One-shot: trigger minion eject this tick.
    pub minion_eject: bool,
    /// Minion frozen: stop minion movement.
    pub minion_frozen: bool,
    /// Minion collect: seek nearest food.
    pub minion_collect: bool,
    /// XRay mode: see all player cells (operator only).
    pub xray_enabled: bool,
    /// Player frozen: main cells stop moving toward mouse (minions unaffected).
    pub frozen: bool,
}

impl Client {
    /// Create a new client session.
    pub fn new(id: u32, addr: SocketAddr) -> Self {
        use rand::Rng;
        let mut rng = rand::rng();

        Self {
            id,
            addr,
            protocol: 0,
            handshake_complete: false,
            name: String::new(),
            skin: None,
            color: Color::new(
                rng.random_range(50..=255),
                rng.random_range(50..=255),
                rng.random_range(50..=255),
            ),
            mouse_x: 0,
            mouse_y: 0,
            cells: Vec::new(),
            scramble_id: rng.random(),
            scramble_x: rng.random_range(-1000..1000),
            scramble_y: rng.random_range(-1000..1000),
            is_spectating: false,
            is_operator: false,
            last_activity: std::time::Instant::now(),
            center_x: 0.0,
            center_y: 0.0,
            scale: 1.0,
            view_min_x: 0.0,
            view_min_y: 0.0,
            view_max_x: 0.0,
            view_max_y: 0.0,
            client_nodes: HashSet::new(),
            view_nodes: Vec::new(),
            needs_border: true,
            leaderboard_tick: 0,
            last_eject_tick: 0,
            last_stat_tick: 0,
            team: None,
            minion_control: false,
            minions: Vec::new(),
            latest_minion_id: 0,
            minion_follow: false,
            minion_split: false,
            minion_eject: false,
            minion_frozen: false,
            minion_collect: false,
            xray_enabled: false,
            frozen: false,
        }
    }

    /// Update activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    /// Get the player's total mass.
    pub fn get_total_size(&self) -> f32 {
        // This would sum all cell sizes, but we need access to the world
        0.0 // Placeholder
    }

    /// Update the center position based on owned cells.
    pub fn update_center(&mut self, positions: &[(f32, f32)]) {
        if positions.is_empty() {
            return;
        }
        let mut cx = 0.0;
        let mut cy = 0.0; 
        for (x, y) in positions {
            cx += x;
            cy += y;
        }
        self.center_x = cx / positions.len() as f32;
        self.center_y = cy / positions.len() as f32;
    }

    /// Update the viewport based on scale.
    pub fn update_viewport(&mut self, view_base_x: f32, view_base_y: f32, min_scale: f32) {
        let scale = self.scale.max(min_scale);
        let half_width = (view_base_x / scale) / 2.0;
        let half_height = (view_base_y / scale) / 2.0;

        self.view_min_x = self.center_x - half_width;
        self.view_min_y = self.center_y - half_height;
        self.view_max_x = self.center_x + half_width;
        self.view_max_y = self.center_y + half_height;
    }

    /// Update scale based on total cell size (from JS: Math.pow(Math.min(64 / totalSize, 1), 0.4)).
    pub fn update_scale(&mut self, total_size: f32) {
        if total_size <= 0.0 {
            self.scale = 1.0;
        } else {
            self.scale = (64.0 / total_size).min(1.0).powf(0.4);
        }
    }
}

