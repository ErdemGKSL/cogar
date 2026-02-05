use super::GameMode;
use crate::server::client::Client;
use crate::server::LeaderboardEntry;
use crate::world::World;
use crate::ai::BotManager;
use protocol::Color;
use std::collections::HashMap;

pub struct Rainbow {
    colors: Vec<Color>,
    speed: usize,
    /// Track color index for each cell ID
    cell_indices: HashMap<u32, usize>,
}

impl Rainbow {
    pub fn new() -> Self {
        let colors = vec![
            Color::new(255, 0, 0),    // Red
            Color::new(255, 32, 0),
            Color::new(255, 64, 0),
            Color::new(255, 96, 0),
            Color::new(255, 128, 0),  // Orange
            Color::new(255, 160, 0),
            Color::new(255, 192, 0),
            Color::new(255, 224, 0),
            Color::new(255, 255, 0),  // Yellow
            Color::new(192, 255, 0),
            Color::new(128, 255, 0),
            Color::new(64, 255, 0),
            Color::new(0, 255, 0),    // Green
            Color::new(0, 192, 64),
            Color::new(0, 128, 128),
            Color::new(0, 64, 192),
            Color::new(0, 0, 255),    // Blue
            Color::new(18, 0, 192),
            Color::new(37, 0, 128),
            Color::new(56, 0, 64),
            Color::new(75, 0, 130),   // Indigo
            Color::new(92, 0, 161),
            Color::new(109, 0, 192),
            Color::new(126, 0, 223),
            Color::new(143, 0, 255),  // Purple
            Color::new(171, 0, 192),
            Color::new(199, 0, 128),
            Color::new(227, 0, 64),
        ];

        Self {
            colors,
            speed: 1,
            cell_indices: HashMap::new(),
        }
    }
}

impl GameMode for Rainbow {
    fn name(&self) -> &str {
        "Rainbow FFA"
    }

    fn id(&self) -> u32 {
        3
    }

    fn on_player_join(&self, _client: &mut Client) {
        // Standard FFA
    }

    fn on_player_spawn(&self, _client: &mut Client) {
        // Standard FFA
    }

    fn on_bot_spawn(&self, _bot: &mut crate::ai::bot_player::Bot) {
        // Standard FFA
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, _clients: &HashMap<u32, Client>, _bots: &BotManager) -> bool {
        owner_id != other_owner_id
    }

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        // Standard FFA Leaderboard
        super::ffa::Ffa::new().get_leaderboard(world, clients, bots)
    }

    fn on_tick(&mut self, game_state: &mut crate::server::game::GameState) {
        let world = &mut game_state.world;
        
        let all_ids: Vec<u32> = world.cells.keys().copied().collect();
        
        for id in all_ids {
            // Get or init index
            let index = self.cell_indices.entry(id).or_insert_with(|| {
                use rand::Rng;
                let mut rng = rand::rng();
                rng.random_range(0..self.colors.len())
            });
            
            // Update color
            let color = self.colors[*index];
            
            if let Some(cell) = world.get_cell_mut(id) {
                cell.data_mut().color = color;
            }
            
            // Advance index
            *index += self.speed;
            if *index >= self.colors.len() {
                *index = 0;
            }
        }
        
        // Clean up indices for removed cells
        if self.cell_indices.len() > world.cells.len() + 100 {
             self.cell_indices.retain(|k, _| world.cells.contains_key(k));
        }
    }
}
