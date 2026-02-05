use super::GameMode;
use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;
use rand::Rng;

pub struct Teams;

impl Teams {
    pub fn new() -> Self {
        Self
    }

    fn get_team_color(&self, team: u8) -> protocol::Color {
        let mut rng = rand::rng();
        let fuzz = 38;
        
        let base_color = match team {
            0 => (255, 0, 0), // Red
            1 => (0, 255, 0), // Green
            _ => (0, 0, 255), // Blue
        };

        let r = (base_color.0 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;
        let g = (base_color.1 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;
        let b = (base_color.2 as i32 + rng.random_range(0..fuzz)).clamp(0, 255) as u8;

        protocol::Color::new(r, g, b)
    }
}

impl GameMode for Teams {
    fn name(&self) -> &str { "Teams" }
    fn id(&self) -> u32 { 1 }

    fn on_player_join(&self, client: &mut Client) {
        if client.team.is_none() {
            let mut rng = rand::rng();
            let team = rng.random_range(0..3);
            client.team = Some(team);
        }
        
        if let Some(team) = client.team {
            client.color = self.get_team_color(team);
        }
    }

    fn on_player_spawn(&self, client: &mut Client) {
        if let Some(team) = client.team {
            client.color = self.get_team_color(team);
        }
    }

    fn on_bot_spawn(&self, bot: &mut crate::ai::bot_player::Bot) {
        if bot.team.is_none() {
            let mut rng = rand::rng();
            let team = rng.random_range(0..3);
            bot.team = Some(team);
        }
        if let Some(team) = bot.team {
            bot.color = self.get_team_color(team);
        }
    }

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, clients: &HashMap<u32, Client>, bots: &BotManager) -> bool {
        if owner_id == other_owner_id { return true; }

        let team_a = if let Some(c) = clients.get(&owner_id) { c.team } else if let Some(b) = bots.get_bot(owner_id) { b.team } else { None };
        let team_b = if let Some(c) = clients.get(&other_owner_id) { c.team } else if let Some(b) = bots.get_bot(other_owner_id) { b.team } else { None };

        match (team_a, team_b) {
            (Some(ta), Some(tb)) => ta != tb,
            _ => true,
        }
    }

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry> {
        let mut team_mass = [0.0; 3];
        let mut total_mass = 0.0;

        for entry in world.iter_cells() {
            if let crate::world::CellEntry::Player(_) = entry.1 {
                let data = entry.1.data();
                let mass = data.size * data.size / 100.0;
                
                let team = if let Some(owner_id) = data.owner_id {
                    if let Some(client) = clients.get(&owner_id) {
                        client.team
                    } else if let Some(bot) = bots.get_bot(owner_id) {
                        bot.team
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(t) = team {
                    if (t as usize) < 3 {
                        team_mass[t as usize] += mass;
                    }
                }
                total_mass += mass;
            }
        }

        let mut entries = Vec::new();
        if total_mass > 0.0 {
            for i in 0..3 {
                entries.push(LeaderboardEntry {
                    client_id: i as u32,
                    name: format!("Team {}", i), // In JS these aren't really names sent in LB packet, but we'll use them here
                    score: team_mass[i] / total_mass,
                });
            }
        }
        entries
    }
}
