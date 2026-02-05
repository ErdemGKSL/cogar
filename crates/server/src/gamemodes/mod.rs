use crate::server::client::Client;
use crate::world::World;
use crate::ai::BotManager;
use crate::server::LeaderboardEntry;
use std::collections::HashMap;

pub mod ffa;
pub mod teams;
pub mod experimental;
pub mod rainbow;
pub mod tournament;
pub mod hunger_games;
pub mod beatdown;


pub trait GameMode: Send + Sync {
    fn name(&self) -> &str;
    fn id(&self) -> u32;

    fn on_player_join(&self, client: &mut Client);
    fn on_player_spawn(&self, client: &mut Client);
    fn on_bot_spawn(&self, bot: &mut crate::ai::bot_player::Bot);

    fn can_eat(&self, owner_id: u32, other_owner_id: u32, clients: &HashMap<u32, Client>, bots: &BotManager) -> bool;

    fn get_leaderboard(&self, world: &World, clients: &HashMap<u32, Client>, bots: &BotManager) -> Vec<LeaderboardEntry>;

    fn on_tick(&mut self, _game_state: &mut crate::server::game::GameState) {}

    /// Called when a player/bot is killed. Default: no-op.
    fn on_player_death(&mut self, _game_state: &mut crate::server::game::GameState, _killer_id: u32, _victim_id: u32) {}

    /// Get movement speed multiplier for a player. Default: 1.0.
    fn get_speed_multiplier(&self, _player_id: u32) -> f32 { 1.0 }

    /// Get view range bonus for a player. Default: 0.0.
    fn get_view_bonus(&self, _player_id: u32) -> f32 { 0.0 }
}

pub fn get_gamemode(id: u32) -> Box<dyn GameMode> {
    match id {
        1 => Box::new(teams::Teams::new()),
        2 => Box::new(experimental::Experimental::new()),
        3 => Box::new(rainbow::Rainbow::new()),
        4 => Box::new(tournament::Tournament::new()),
        5 => Box::new(hunger_games::HungerGames::new()),
        6 => Box::new(beatdown::Beatdown::new()),
        _ => Box::new(ffa::Ffa::new()),
    }
}
