//! Beatdown game mode.
//!
//! Kill-based progression mode where players gain speed and view bonuses per kill.
//! Auto-respawn after death. Leaderboard shows kill count.

use super::GameMode;
use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;

/// Beatdown game mode.
pub struct Beatdown {
    /// Kill count per player.
    kill_count: HashMap<u32, u32>,
    /// Speed bonus per kill.
    speed_bonus_per_kill: f32,
    /// View range bonus per kill.
    view_bonus_per_kill: f32,
    /// Max speed bonus.
    max_speed_bonus: f32,
    /// Max view bonus.
    max_view_bonus: f32,
}

impl Beatdown {
    pub fn new() -> Self {
        Self {
            kill_count: HashMap::new(),
            speed_bonus_per_kill: 0.05, // 5% speed increase per kill
            view_bonus_per_kill: 50.0,  // 50 units view range per kill
            max_speed_bonus: 0.5,       // Max 50% speed bonus
            max_view_bonus: 500.0,      // Max 500 view range bonus
        }
    }

    /// Record a kill for a player.
    pub fn record_kill(&mut self, killer_id: u32) {
        *self.kill_count.entry(killer_id).or_insert(0) += 1;
    }

    /// Get kill count for a player.
    pub fn get_kills(&self, player_id: u32) -> u32 {
        self.kill_count.get(&player_id).copied().unwrap_or(0)
    }

    /// Get speed multiplier for a player based on kills.
    pub fn get_speed_multiplier(&self, player_id: u32) -> f32 {
        let kills = self.get_kills(player_id) as f32;
        let bonus = (kills * self.speed_bonus_per_kill).min(self.max_speed_bonus);
        1.0 + bonus
    }

    /// Get view range bonus for a player based on kills.
    pub fn get_view_bonus(&self, player_id: u32) -> f32 {
        let kills = self.get_kills(player_id) as f32;
        (kills * self.view_bonus_per_kill).min(self.max_view_bonus)
    }

    /// Reset kill count for a player (on death).
    pub fn reset_kills(&mut self, player_id: u32) {
        self.kill_count.remove(&player_id);
    }

    /// Clear all kill counts.
    pub fn clear(&mut self) {
        self.kill_count.clear();
    }
}

impl Default for Beatdown {
    fn default() -> Self {
        Self::new()
    }
}

impl GameMode for Beatdown {
    fn name(&self) -> &str {
        "Beatdown"
    }

    fn id(&self) -> u32 {
        6
    }

    fn on_player_join(&self, _client: &mut Client) {
        // Standard FFA join
    }

    fn on_player_spawn(&self, _client: &mut Client) {
        // Standard FFA spawn - speed bonus applied elsewhere
    }

    fn on_bot_spawn(&self, _bot: &mut crate::ai::bot_player::Bot) {
        // Standard FFA bot spawn
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, _clients: &HashMap<u32, Client>, _bots: &BotManager) -> bool {
        // FFA eating rules
        owner_id != other_owner_id
    }

    fn get_leaderboard(&self, _world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        // Leaderboard sorted by kill count
        let mut entries: Vec<LeaderboardEntry> = clients.iter()
            .filter(|(_, client)| !client.cells.is_empty())
            .map(|(&client_id, client)| {
                let kills = self.get_kills(client_id);
                LeaderboardEntry {
                    client_id,
                    name: if client.name.is_empty() {
                        "An unnamed cell".to_string()
                    } else {
                        client.name.clone()
                    },
                    score: kills as f32,
                }
            })
            .collect();

        // Add bots (but not minions)
        for bot in &bots.bots {
            if bot.cells.is_empty() {
                continue;
            }

            // Skip if this bot is a minion owned by any client
            let is_minion = clients.values().any(|client| client.minions.contains(&bot.id));
            if is_minion {
                continue;
            }

            let kills = self.get_kills(bot.id);
            entries.push(LeaderboardEntry {
                client_id: bot.id,
                name: bot.name.clone(),
                score: kills as f32,
            });
        }

        entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }

    fn on_tick(&mut self, _game_state: &mut crate::server::game::GameState) {}

    fn on_player_death(&mut self, game_state: &mut crate::server::game::GameState, killer_id: u32, victim_id: u32) {
        self.record_kill(killer_id);
        self.reset_kills(victim_id);
        
        // Respawn victim immediately
        if game_state.clients.contains_key(&victim_id) {
            game_state.spawn_player(victim_id);
        }
        // Check if it's a bot
        else if let Some(bot) = game_state.bots.get_bot_mut(victim_id) {
            bot.cells.clear();
            bot.needs_respawn = true;
        }
    }

    fn get_speed_multiplier(&self, player_id: u32) -> f32 {
        self.get_speed_multiplier(player_id)
    }

    fn get_view_bonus(&self, player_id: u32) -> f32 {
        self.get_view_bonus(player_id)
    }
}
