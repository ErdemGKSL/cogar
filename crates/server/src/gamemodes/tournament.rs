//! Tournament game mode.
//!
//! Phase-based tournament with waiting lobby, preparation time, and winner declaration.

use super::GameMode;
use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;

/// Tournament phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentPhase {
    /// Waiting for players (phase 0)
    Waiting = 0,
    /// Preparation time before game starts (phase 1)
    Preparing = 1,
    /// Game in progress (phase 2)
    Active = 2,
    /// Winner declared (phase 3)
    Winner = 3,
    /// Timeout/reset (phase 4)
    Timeout = 4,
}

impl From<u8> for TournamentPhase {
    fn from(val: u8) -> Self {
        match val {
            0 => TournamentPhase::Waiting,
            1 => TournamentPhase::Preparing,
            2 => TournamentPhase::Active,
            3 => TournamentPhase::Winner,
            4 => TournamentPhase::Timeout,
            _ => TournamentPhase::Waiting,
        }
    }
}

/// Tournament game mode.
pub struct Tournament {
    /// Current phase.
    pub phase: TournamentPhase,
    /// List of contender client IDs.
    pub contenders: Vec<u32>,
    /// Timer ticks for current phase.
    pub timer: u64,
    /// Minimum players to start.
    pub min_players: usize,
    /// Preparation time in ticks.
    pub prepare_time: u64,
    /// Time after winner before reset in ticks.
    pub winner_time: u64,
    /// Auto-fill with bots.
    pub auto_fill: bool,
    /// Target player count for auto-fill.
    pub auto_fill_count: usize,
}

impl Tournament {
    pub fn new() -> Self {
        Self {
            phase: TournamentPhase::Waiting,
            contenders: Vec::new(),
            timer: 0,
            min_players: 2,
            prepare_time: 100, // ~4 seconds at 25 TPS
            winner_time: 250,  // ~10 seconds
            auto_fill: false,
            auto_fill_count: 5,
        }
    }

    /// Add a contender to the tournament.
    pub fn add_contender(&mut self, client_id: u32) {
        if !self.contenders.contains(&client_id) {
            self.contenders.push(client_id);
        }
    }

    /// Remove a contender from the tournament.
    pub fn remove_contender(&mut self, client_id: u32) {
        self.contenders.retain(|&id| id != client_id);
    }

    /// Get number of alive contenders.
    pub fn alive_count(&self, clients: &HashMap<u32, Client>, bots: &BotManager) -> usize {
        self.contenders.iter().filter(|&&id| {
            if let Some(c) = clients.get(&id) {
                !c.cells.is_empty()
            } else if let Some(b) = bots.get_bot(id) {
                !b.cells.is_empty()
            } else {
                false
            }
        }).count()
    }

    /// Check if a client/bot is a contender.
    pub fn is_contender(&self, id: u32) -> bool {
        self.contenders.contains(&id)
    }

    /// Reset tournament to waiting phase.
    pub fn reset(&mut self) {
        self.phase = TournamentPhase::Waiting;
        self.contenders.clear();
        self.timer = 0;
    }

    /// Get the winner (last alive contender).
    pub fn get_winner(&self, clients: &HashMap<u32, Client>, bots: &BotManager) -> Option<u32> {
        for &id in &self.contenders {
            if let Some(c) = clients.get(&id) {
                if !c.cells.is_empty() {
                    return Some(id);
                }
            } else if let Some(b) = bots.get_bot(id) {
                if !b.cells.is_empty() {
                    return Some(id);
                }
            }
        }
        None
    }
}

impl Default for Tournament {
    fn default() -> Self {
        Self::new()
    }
}

impl GameMode for Tournament {
    fn name(&self) -> &str {
        "Tournament"
    }

    fn id(&self) -> u32 {
        4
    }

    fn on_player_join(&self, _client: &mut Client) {
        // Players start as spectators until they become contenders
    }

    fn on_player_spawn(&self, _client: &mut Client) {
        // Spawning handled by on_tick based on phase
    }

    fn on_bot_spawn(&self, _bot: &mut crate::ai::bot_player::Bot) {
        // Bots handled same as players
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, _clients: &HashMap<u32, Client>, _bots: &BotManager) -> bool {
        // FFA eating rules
        owner_id != other_owner_id
    }

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        // Only show contenders on leaderboard
        let mut entries: Vec<LeaderboardEntry> = self.contenders.iter()
            .filter_map(|&client_id| {
                let (name, cells) = if let Some(c) = clients.get(&client_id) {
                    (c.name.clone(), &c.cells)
                } else if let Some(b) = bots.get_bot(client_id) {
                    (b.name.clone(), &b.cells)
                } else {
                    return None;
                };

                if cells.is_empty() {
                    return None;
                }

                let score: f32 = cells.iter()
                    .filter_map(|&cell_id| world.get_cell(cell_id))
                    .map(|cell| {
                        let size = cell.data().size;
                        size * size / 100.0
                    })
                    .sum();

                Some(LeaderboardEntry {
                    client_id,
                    name: if name.is_empty() { "An unnamed cell".to_string() } else { name },
                    score,
                })
            })
            .collect();

        entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }

    fn on_tick(&mut self, game_state: &mut crate::server::game::GameState) {
        let clients = &mut game_state.clients;
        let bots = &mut game_state.bots;
        self.timer += 1;

        match self.phase {
            TournamentPhase::Waiting => {
                // Add new players as contenders
                for (&id, client) in clients.iter() {
                    if !self.is_contender(id) && !client.is_spectating {
                        self.add_contender(id);
                    }
                }

                // Add bots as contenders (but not minions)
                for bot in &bots.bots {
                    if !self.is_contender(bot.id) {
                        // Skip if this bot is a minion owned by any client
                        let is_minion = clients.values().any(|client| client.minions.contains(&bot.id));
                        if !is_minion {
                            self.add_contender(bot.id);
                        }
                    }
                }

                // Check if enough players to start
                if self.contenders.len() >= self.min_players {
                    self.phase = TournamentPhase::Preparing;
                    self.timer = 0;
                    tracing::info!("Tournament: Starting preparation phase with {} contenders", self.contenders.len());
                }
            }

            TournamentPhase::Preparing => {
                // Wait for preparation time
                if self.timer >= self.prepare_time {
                    self.phase = TournamentPhase::Active;
                    self.timer = 0;
                    tracing::info!("Tournament: Game started!");
                }
            }

            TournamentPhase::Active => {
                let alive = self.alive_count(clients, bots);

                if alive == 0 {
                    // No one alive - timeout
                    self.phase = TournamentPhase::Timeout;
                    self.timer = 0;
                } else if alive == 1 {
                    if let Some(winner_id) = self.get_winner(clients, bots) {
                        let winner_name = if let Some(c) = clients.get(&winner_id) {
                            c.name.clone()
                        } else if let Some(b) = bots.get_bot(winner_id) {
                            b.name.clone()
                        } else {
                            "Unknown".to_string()
                        };
                        tracing::info!("Tournament: Winner is {}!", winner_name);
                    }
                    self.phase = TournamentPhase::Winner;
                    self.timer = 0;
                }
            }

            TournamentPhase::Winner | TournamentPhase::Timeout => {
                // Wait then reset
                if self.timer >= self.winner_time {
                    tracing::info!("Tournament: Resetting for new round");
                    self.reset();
                }
            }
        }
    }
}
