use super::GameMode;
use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;

pub struct Ffa;

impl Ffa {
    pub fn new() -> Self {
        Self
    }
}

impl GameMode for Ffa {
    fn name(&self) -> &str { "FFA" }
    fn id(&self) -> u32 { 0 }

    fn on_player_join(&self, _client: &mut Client) {
        // No special logic for FFA join
    }

    fn on_player_spawn(&self, _client: &mut Client) {
        // No special logic for FFA spawn
    }

    fn on_bot_spawn(&self, _bot: &mut crate::ai::bot_player::Bot) {
        // No special logic for FFA bot spawn
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, _clients: &HashMap<u32, Client>, _bots: &BotManager) -> bool {
        owner_id != other_owner_id
    }

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        let mut entries: Vec<LeaderboardEntry> = clients
            .iter()
            .filter(|(_, client)| !client.cells.is_empty())
            .map(|(&client_id, client)| {
                let score: f32 = client
                    .cells
                    .iter()
                    .filter_map(|&cell_id| world.get_cell(cell_id))
                    .map(|cell| {
                        let size = cell.data().size;
                        size * size / 100.0
                    })
                    .sum();

                LeaderboardEntry {
                    client_id,
                    name: if client.name.is_empty() {
                        "An unnamed cell".to_string()
                    } else {
                        client.name.clone()
                    },
                    score,
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
            
            let score: f32 = bot.cells.iter()
                .filter_map(|&id| world.get_cell(id))
                .map(|c| c.data().size * c.data().size / 100.0)
                .sum();
            
            entries.push(LeaderboardEntry {
                client_id: bot.id,
                name: bot.name.clone(),
                score,
            });
        }

        entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }
}
