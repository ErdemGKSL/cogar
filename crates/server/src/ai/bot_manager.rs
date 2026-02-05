use super::bot_player::Bot;
use crate::config::Config;
use crate::world::World;

/// Bot manager.
#[derive(Debug, Default)]
pub struct BotManager {
    /// Active bots.
    pub bots: Vec<Bot>,
    /// Next bot ID counter.
    next_id: u32,
}

impl BotManager {
    /// Create a new bot manager.
    pub fn new() -> Self {
        Self {
            bots: Vec::new(),
            next_id: 1_000_000, // Start bot IDs high to avoid collision with real clients
        }
    }

    /// Add a new bot.
    pub fn add_bot(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.bots.push(Bot::new(id));
        id
    }

    /// Remove a bot by ID.
    pub fn remove_bot(&mut self, id: u32) {
        self.bots.retain(|b| b.id != id);
    }

    /// Get a bot by ID.
    pub fn get_bot(&self, id: u32) -> Option<&Bot> {
        self.bots.iter().find(|b| b.id == id)
    }

    /// Get a mutable bot by ID.
    pub fn get_bot_mut(&mut self, id: u32) -> Option<&mut Bot> {
        self.bots.iter_mut().find(|b| b.id == id)
    }

    /// Update all bots, skipping any IDs in `skip` (used for minions, which are
    /// controlled by their owner rather than running independent AI).
    pub fn update(&mut self, world: &mut World, config: &Config, team_lookup: &std::collections::HashMap<u32, u8>, skip: &std::collections::HashSet<u32>) {
        for bot in &mut self.bots {
            if skip.contains(&bot.id) {
                continue;
            }
            bot.update(world, config, team_lookup);
        }
    }

    /// Get bot IDs that need to respawn.
    pub fn get_respawn_list(&self) -> Vec<u32> {
        self.bots
            .iter()
            .filter(|b| b.needs_respawn || b.cells.is_empty())
            .map(|b| b.id)
            .collect()
    }
}
