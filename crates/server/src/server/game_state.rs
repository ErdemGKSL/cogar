use crate::config::Config;
use crate::world::World;
use crate::bot::BotManager;
use crate::server::client::Client;
use crate::gamemodes::{self, GameMode};
use protocol::{TargetedMessage, LeaderboardBroadcast, WorldBroadcast};
use tokio::sync::{mpsc, broadcast};
use std::collections::HashMap;
use tracing::{info, debug, warn};

pub struct GameState {
    pub config: Config,
    pub world: World,
    pub clients: HashMap<u32, Client>,
    pub bots: BotManager,
    pub tick_count: u64,
    pub gamemode: Box<dyn GameMode>,

    // Channels
    pub chat_tx: broadcast::Sender<protocol::ChatMessage>,
    pub lb_tx: broadcast::Sender<LeaderboardBroadcast>,
    pub world_tx: broadcast::Sender<WorldBroadcast>,
    pub targeted_tx: mpsc::UnboundedSender<TargetedMessage>,
}

impl GameState {
    pub fn new(
        config: Config,
        chat_tx: broadcast::Sender<protocol::ChatMessage>,
        lb_tx: broadcast::Sender<LeaderboardBroadcast>,
        world_tx: broadcast::Sender<WorldBroadcast>,
        targeted_tx: mpsc::UnboundedSender<TargetedMessage>,
    ) -> Self {
        let gamemode_id = config.server.gamemode;
        let mut world = World::new(config.border.width as f32, config.border.height as f32);
        
        // Initial world setup based on config
        world.spawn_food(config.food.min_amount);
        world.spawn_viruses(config.virus.min_amount);

        Self {
            gamemode: gamemodes::get_gamemode(gamemode_id),
            config,
            world,
            clients: HashMap::new(),
            bots: BotManager::new(),
            tick_count: 0,
            chat_tx,
            lb_tx,
            world_tx,
            targeted_tx,
        }
    }
}
