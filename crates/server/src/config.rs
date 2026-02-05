//! Server configuration.

use serde::{Deserialize, Serialize};
use tracing::info;
use std::path::Path;

/// Root configuration structure.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub border: BorderConfig,
    #[serde(default)]
    pub player: PlayerConfig,
    #[serde(default)]
    pub food: FoodConfig,
    #[serde(default)]
    pub virus: VirusConfig,
    #[serde(default)]
    pub eject: EjectConfig,
}

impl Config {
    /// Load configuration from `config.toml` or use defaults.
    pub fn load() -> anyhow::Result<Self> {
        let path = Path::new("config.toml");
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&contents)?)
        } else {
            info!("No config.toml found, creating default config");
            let default_config = Self::default();
            std::fs::write(path, toml::to_string_pretty(&default_config)?)?;
            Ok(default_config)
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            border: BorderConfig::default(),
            player: PlayerConfig::default(),
            food: FoodConfig::default(),
            virus: VirusConfig::default(),
            eject: EjectConfig::default(),
        }
    }
}

/// Server networking and general settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Port to listen on.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Bind address.
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Maximum connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// Connection timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Connections per IP limit.
    #[serde(default = "default_ip_limit")]
    pub ip_limit: usize,
    /// Game mode (0=FFA, 1=Teams, 2=Experimental, etc.)
    #[serde(default)]
    pub gamemode: u32,
    /// Server name shown to clients.
    #[serde(default = "default_name")]
    pub name: String,
    /// Tick interval in milliseconds.
    #[serde(default = "default_tick_interval")]
    pub tick_interval_ms: u64,
    /// Number of bots to spawn.
    #[serde(default)]
    pub bots: usize,
    /// Number of default minions to give each player.
    #[serde(default)]
    pub server_minions: usize,
    /// Enable mobile physics (looser eat threshold, faster remerge, no auto-split).
    #[serde(default = "default_mobile_physics")]
    pub mobile_physics: bool,
    /// Password to toggle operator mode (empty = operator disabled).
    #[serde(default)]
    pub operator_password: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            bind: default_bind(),
            max_connections: default_max_connections(),
            timeout: default_timeout(),
            ip_limit: default_ip_limit(),
            gamemode: 0,
            name: default_name(),
            tick_interval_ms: default_tick_interval(),
            bots: 0,
            server_minions: 0,
            mobile_physics: default_mobile_physics(),
            operator_password: String::new(),
        }
    }
}

fn default_port() -> u16 {
    11443
}
fn default_bind() -> String {
    "0.0.0.0".to_string()
}
fn default_max_connections() -> usize {
    100
}
fn default_timeout() -> u64 {
    300
}
fn default_ip_limit() -> usize {
    100
}
fn default_name() -> String {
    "Native Ogar".to_string()
}
fn default_mobile_physics() -> bool {
    true
}
fn default_tick_interval() -> u64 {
    40
}

/// World border configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BorderConfig {
    #[serde(default = "default_border_size")]
    pub width: f64,
    #[serde(default = "default_border_size")]
    pub height: f64,
}

impl Default for BorderConfig {
    fn default() -> Self {
        Self {
            width: default_border_size(),
            height: default_border_size(),
        }
    }
}

fn default_border_size() -> f64 {
    14142.0
}

/// Player configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlayerConfig {
    #[serde(default = "default_player_start_size")]
    pub start_size: f64,
    #[serde(default = "default_player_min_size")]
    pub min_size: f64,
    #[serde(default = "default_player_max_size")]
    pub max_size: f64,
    #[serde(default = "default_player_min_split")]
    pub min_split_size: f64,
    #[serde(default = "default_player_min_eject")]
    pub min_eject_size: f64,
    #[serde(default = "default_player_max_cells")]
    pub max_cells: usize,
    #[serde(default = "default_player_speed")]
    pub speed: f64,
    #[serde(default = "default_player_decay_rate")]
    pub decay_rate: f64,
    #[serde(default = "default_player_merge_time")]
    pub merge_time: f64,
    #[serde(default = "default_player_split_speed")]
    pub split_speed: f64,
    #[serde(default)]
    pub minion_same_color: bool,
    #[serde(default = "default_max_nick_length")]
    pub max_nick_length: usize,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            start_size: default_player_start_size(),
            min_size: default_player_min_size(),
            max_size: default_player_max_size(),
            min_split_size: default_player_min_split(),
            min_eject_size: default_player_min_eject(),
            max_cells: default_player_max_cells(),
            speed: default_player_speed(),
            decay_rate: default_player_decay_rate(),
            merge_time: default_player_merge_time(),
            split_speed: default_player_split_speed(),
            minion_same_color: false,
            max_nick_length: default_max_nick_length(),
        }
    }
}

fn default_player_start_size() -> f64 {
    30.0
}
fn default_player_min_size() -> f64 {
    30.0
}
fn default_player_max_size() -> f64 {
    1500.0
}
fn default_player_min_split() -> f64 {
    60.0
}
fn default_player_min_eject() -> f64 {
    60.0
}
fn default_player_max_cells() -> usize {
    16
}
fn default_player_speed() -> f64 {
    30.0
}
fn default_player_decay_rate() -> f64 {
    0.002
}
fn default_player_merge_time() -> f64 {
    30.0
}
fn default_player_split_speed() -> f64 {
    780.0
}
fn default_max_nick_length() -> usize {
    30
}

/// Food configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FoodConfig {
    #[serde(default = "default_food_size")]
    pub min_size: f64,
    #[serde(default = "default_food_size")]
    pub max_size: f64,
    #[serde(default = "default_food_min_amount")]
    pub min_amount: usize,
    #[serde(default = "default_food_max_amount")]
    pub max_amount: usize,
    #[serde(default = "default_food_spawn_amount")]
    pub spawn_amount: usize,
}

impl Default for FoodConfig {
    fn default() -> Self {
        Self {
            min_size: default_food_size(),
            max_size: default_food_size(),
            min_amount: default_food_min_amount(),
            max_amount: default_food_max_amount(),
            spawn_amount: default_food_spawn_amount(),
        }
    }
}

fn default_food_size() -> f64 {
    10.0
}
fn default_food_min_amount() -> usize {
    1500
}
fn default_food_max_amount() -> usize {
    3000
}
fn default_food_spawn_amount() -> usize {
    30
}

/// Virus configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VirusConfig {
    #[serde(default = "default_virus_min_size")]
    pub min_size: f64,
    #[serde(default = "default_virus_max_size")]
    pub max_size: f64,
    #[serde(default = "default_virus_min_amount")]
    pub min_amount: usize,
    #[serde(default = "default_virus_max_amount")]
    pub max_amount: usize,
    #[serde(default = "default_virus_eject_speed")]
    pub eject_speed: f64,
    /// Maximum total cells a player can have after a virus pop
    /// (JS: virusMaxCells, falls back to playerMaxCells when unset).
    #[serde(default = "default_virus_max_cells")]
    pub max_cells: usize,
    /// Minimum mass per split piece when a virus pops a player
    /// (JS: virusSplitDiv).  Controls how many pieces result: a higher
    /// value produces fewer, larger pieces.
    #[serde(default = "default_virus_split_div")]
    pub split_div: f64,
}

impl Default for VirusConfig {
    fn default() -> Self {
        Self {
            min_size: default_virus_min_size(),
            max_size: default_virus_max_size(),
            min_amount: default_virus_min_amount(),
            max_amount: default_virus_max_amount(),
            eject_speed: default_virus_eject_speed(),
            max_cells: default_virus_max_cells(),
            split_div: default_virus_split_div(),
        }
    }
}

fn default_virus_min_size() -> f64 {
    100.0
}
fn default_virus_max_size() -> f64 {
    141.4
}
fn default_virus_min_amount() -> usize {
    50
}
fn default_virus_max_amount() -> usize {
    100
}
fn default_virus_eject_speed() -> f64 {
    780.0
}
fn default_virus_max_cells() -> usize {
    12
}
fn default_virus_split_div() -> f64 {
    36.0
}

/// Ejected mass configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EjectConfig {
    #[serde(default = "default_eject_size")]
    pub size: f64,
    #[serde(default = "default_eject_size_loss")]
    pub size_loss: f64,
    #[serde(default = "default_eject_speed")]
    pub speed: f64,
    #[serde(default = "default_eject_cooldown")]
    pub cooldown: u32,
}

impl Default for EjectConfig {
    fn default() -> Self {
        Self {
            size: default_eject_size(),
            size_loss: default_eject_size_loss(),
            speed: default_eject_speed(),
            cooldown: default_eject_cooldown(),
        }
    }
}

fn default_eject_size() -> f64 {
    36.056
}
fn default_eject_size_loss() -> f64 {
    41.231
}
fn default_eject_speed() -> f64 {
    780.0
}
fn default_eject_cooldown() -> u32 {
    2
}
