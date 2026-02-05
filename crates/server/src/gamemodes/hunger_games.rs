//! Hunger Games game mode.
//!
//! Tournament-style mode with predefined spawn points around the map edges.
//! Players spawn in set positions and fight until one remains.

use super::GameMode;
use super::tournament::{Tournament, TournamentPhase};
use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;
use glam::Vec2;

/// Hunger Games game mode.
pub struct HungerGames {
    /// Tournament base logic.
    tournament: Tournament,
    /// Predefined spawn points.
    spawn_points: Vec<Vec2>,
    /// Next spawn point index.
    next_spawn_index: usize,
}

impl HungerGames {
    pub fn new() -> Self {
        Self {
            tournament: Tournament::new(),
            spawn_points: Vec::new(),
            next_spawn_index: 0,
        }
    }

    /// Initialize spawn points around the map border.
    /// Creates 12 spawn points evenly distributed around the perimeter.
    pub fn init_spawn_points(&mut self, world: &World) {
        self.spawn_points.clear();

        let border = &world.border;
        let width = border.max_x - border.min_x;
        let height = border.max_y - border.min_y;
        let center_x = (border.min_x + border.max_x) / 2.0;
        let center_y = (border.min_y + border.max_y) / 2.0;

        // Create spawn points around the border (similar to JS version)
        // 12 points evenly distributed
        let num_points = 12;
        let margin = 200.0; // Distance from border edge

        for i in 0..num_points {
            let angle = (i as f32 / num_points as f32) * std::f32::consts::TAU;
            let radius_x = (width / 2.0) - margin;
            let radius_y = (height / 2.0) - margin;

            let x = center_x + radius_x * angle.cos();
            let y = center_y + radius_y * angle.sin();

            self.spawn_points.push(Vec2::new(x, y));
        }

        self.next_spawn_index = 0;
    }

    /// Get the next spawn position.
    pub fn get_spawn_position(&mut self) -> Option<Vec2> {
        if self.spawn_points.is_empty() {
            return None;
        }

        let pos = self.spawn_points[self.next_spawn_index];
        self.next_spawn_index = (self.next_spawn_index + 1) % self.spawn_points.len();
        Some(pos)
    }

    /// Get tournament phase.
    pub fn phase(&self) -> TournamentPhase {
        self.tournament.phase
    }

    /// Check if spawning is allowed.
    pub fn can_spawn(&self) -> bool {
        matches!(self.tournament.phase, TournamentPhase::Waiting | TournamentPhase::Preparing)
    }
}

impl Default for HungerGames {
    fn default() -> Self {
        Self::new()
    }
}

impl GameMode for HungerGames {
    fn name(&self) -> &str {
        "Hunger Games"
    }

    fn id(&self) -> u32 {
        5
    }

    fn on_player_join(&self, _client: &mut Client) {
        // Players will be added as contenders in on_tick
    }

    fn on_player_spawn(&self, _client: &mut Client) {
        // Spawn position will be set from spawn_points
    }

    fn on_bot_spawn(&self, _bot: &mut crate::ai::bot_player::Bot) {
        // Bots handled same as players
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, _clients: &HashMap<u32, Client>, _bots: &BotManager) -> bool {
        // FFA eating rules
        owner_id != other_owner_id
    }

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        self.tournament.get_leaderboard(world, clients, bots)
    }

    fn on_tick(&mut self, game_state: &mut crate::server::game::GameState) {
        let world = &mut game_state.world;
        // Initialize spawn points if needed
        if self.spawn_points.is_empty() {
            self.init_spawn_points(world);
        }

        // Run tournament logic
        self.tournament.on_tick(game_state);

        // In preparing phase, prevent respawning
        if self.tournament.phase == TournamentPhase::Active {
            // Clear respawn flags for dead contenders
            for &id in &self.tournament.contenders {
                if let Some(client) = game_state.clients.get_mut(&id) {
                    if client.cells.is_empty() {
                        // Don't respawn during active game
                        client.is_spectating = true;
                    }
                }
            }
        }

        // Reset spawn index when tournament resets
        if self.tournament.phase == TournamentPhase::Waiting && self.tournament.timer == 0 {
            self.next_spawn_index = 0;
        }
    }
}
