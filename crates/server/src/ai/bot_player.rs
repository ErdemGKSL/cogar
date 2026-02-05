use crate::config::Config;
use crate::entity::CellType;
use crate::world::World;
use glam::Vec2;
use protocol::Color;
use rand::Rng;
use tracing::debug;
use std::collections::HashMap;

/// Bot names to use.
const BOT_NAMES: &[&str] = &[
    "Bot", "Hunter", "Hungry", "Nomnom", "Blob", "Cell", "Eater", "Seeker",
    "Roamer", "Wanderer", "Ghost", "Shadow", "Swift", "Tiny", "Big", "Mega",
];

/// A bot player controlled by AI.
#[derive(Debug)]
pub struct Bot {
    /// Bot ID (same as client_id).
    pub id: u32,
    /// Bot name.
    pub name: String,
    /// Bot color.
    pub color: Color,
    /// Current target position.
    pub target: Vec2,
    /// Cell IDs owned by this bot.
    pub cells: Vec<u32>,
    /// Ticks until next decision.
    pub decision_cooldown: u32,
    /// Whether the bot needs to respawn.
    pub needs_respawn: bool,
    /// Whether the bot wants to split.
    pub split_requested: bool,
    /// Bot team (0=Red, 1=Green, 2=Blue).
    pub team: Option<u8>,
    /// Cooldown for splitting (ticks).
    pub split_cooldown: u32,
    /// Ticks to pursue a split target.
    pub target_pursuit: u32,
    /// ID of the currently pursued target.
    pub split_target_id: Option<u32>,
}

impl Bot {
    /// Create a new bot with the given ID.
    pub fn new(id: u32) -> Self {
        let mut rng = rand::rng();
        let name_idx = rng.random_range(0..BOT_NAMES.len());
        let name = format!("{}{}", BOT_NAMES[name_idx], id % 100);

        Self {
            id,
            name,
            color: World::random_color(),
            target: Vec2::ZERO,
            cells: Vec::new(),
            decision_cooldown: 0,
            needs_respawn: true,
            split_requested: false,
            team: None,
            split_cooldown: 0,
            target_pursuit: 0,
            split_target_id: None,
        }
    }

    /// Update the bot AI.
    pub fn update(&mut self, world: &mut World, config: &Config, team_lookup: &HashMap<u32, u8>) {
        // Reset flags
        self.split_requested = false;

        // Decrement split cooldown
        if self.split_cooldown > 0 {
            self.split_cooldown -= 1;
        }

        // Check if we need to respawn
        if self.cells.is_empty() {
            self.needs_respawn = true;
            return;
        }

        // Ticks until next decision
        if self.decision_cooldown > 0 {
            self.decision_cooldown -= 1;
        }

        // Get our largest cell
        let (my_pos, my_size) = self.get_largest_cell(world);
        if my_size <= 0.0 {
            return;
        }

        // Pursue split target logic
        if let Some(target_id) = self.split_target_id {
            if let Some(target_cell) = world.get_cell(target_id) {
                if self.target_pursuit > 0 {
                    self.target_pursuit -= 1;
                    self.target = target_cell.data().position;
                    return;
                }
            }
            self.split_target_id = None;
            self.target_pursuit = 0;
        }

        if self.decision_cooldown > 0 {
            return;
        }
        self.decision_cooldown = 2;

        let mut result = Vec2::ZERO;
        let mut prey_id: Option<u32> = None;
        let mut prey_size = 0.0;
        let mut prey_pos = Vec2::ZERO;

        let merge = config.player.merge_time as f32 <= 0.0;
        let can_split = (self.cells.len() as f32 * 1.5) < 9.0 && self.split_cooldown == 0;
        let split_size_check = my_size / 1.3;

        // Search radius (view box equivalent)
        let search_radius = 2000.0;
        let nearby = world.find_cells_in_radius(my_pos.x, my_pos.y, search_radius);
        let num_view_nodes = nearby.len().max(1) as f32;

        for &check_id in &nearby {
            if self.cells.contains(&check_id) {
                continue;
            }

            let (check_pos, check_size, check_type, check_owner) = match world.get_cell(check_id) {
                Some(cell) => {
                    let data = cell.data();
                    (data.position, data.size, data.cell_type, data.owner_id)
                }
                None => continue,
            };

            if check_owner == Some(self.id) {
                continue;
            }

            let mut influence = 0.0;
            match check_type {
                CellType::Player => {
                    // Team check
                    if let (Some(my_team), Some(owner_id)) = (self.team, check_owner) {
                        if team_lookup.get(&owner_id) == Some(&my_team) {
                            continue;
                        }
                    }

                    if my_size > check_size * 1.3 {
                        influence = check_size / num_view_nodes.ln().max(1.0);
                    } else if check_size > my_size * 1.3 {
                        influence = -((check_size / my_size).ln());
                    } else {
                        influence = -check_size / my_size;
                    }
                }
                CellType::Food => {
                    influence = 1.0;
                }
                CellType::Virus | CellType::MotherCell => {
                    if my_size > check_size {
                        // Avoid splitting on virus/mother cell
                        influence = -100.0;
                    } else {
                        // Can hide in virus?
                        influence = 0.0;
                    }
                }
                CellType::EjectedMass => {
                    if my_size > check_size * 1.3 {
                        influence = 2.0;
                    }
                }
            }

            if influence != 0.0 {
                let displacement = check_pos - my_pos;
                let mut dist = displacement.length();
                
                if influence < 0.0 {
                    dist -= my_size + check_size;
                }
                
                let dist = dist.max(1.0);
                influence /= dist;
                
                let scale = displacement.normalize() * influence;
                result += scale;

                if can_split && check_type == CellType::Player && split_size_check > check_size {
                    let min_eat_fraction = if merge { 0.1 } else { 0.4 };
                    if my_size * min_eat_fraction < check_size {
                        if self.split_kill(my_size, check_size, dist, config) {
                            if check_size > prey_size {
                                prey_size = check_size;
                                prey_id = Some(check_id);
                                prey_pos = check_pos;
                            }
                        }
                    }
                }
            }
        }

        if let Some(id) = prey_id {
            debug!("Bot {} targeting prey {} (size {}) for split", self.id, id, prey_size);
            self.target = prey_pos;
            self.split_target_id = Some(id);
            self.target_pursuit = if merge { 5 } else { 20 };
            self.split_cooldown = if merge { 5 } else { 15 };
            self.split_requested = true;
        } else {
            if result.length() > 0.01 {
                result = result.normalize();
                self.target = my_pos + result * 2000.0;
            } else {
                let mut rng = rand::rng();
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                self.target = my_pos + Vec2::new(angle.cos(), angle.sin()) * 400.0;
            }
        }

        self.target.x = self.target.x.clamp(world.border.min_x, world.border.max_x);
        self.target.y = self.target.y.clamp(world.border.min_y, world.border.max_y);
    }

    fn split_kill(&self, my_size: f32, _prey_size: f32, dist: f32, config: &Config) -> bool {
        let speed = (1.3 * config.player.split_speed as f32).max(my_size / 1.4142 * 4.5);
        speed >= dist
    }

    fn get_largest_cell(&self, world: &World) -> (Vec2, f32) {
        let mut best_pos = Vec2::ZERO;
        let mut best_size = 0.0;

        for &cell_id in &self.cells {
            if let Some(cell) = world.get_cell(cell_id) {
                let data = cell.data();
                if data.size > best_size {
                    best_size = data.size;
                    best_pos = data.position;
                }
            }
        }

        (best_pos, best_size)
    }
}
