use super::GameMode;
use crate::server::client::Client;
use crate::server::LeaderboardEntry;
use crate::world::{World, CellEntry};
use crate::ai::BotManager;
use crate::entity::{MotherCell, Food, Cell};
use std::collections::HashMap;
use glam::Vec2;
use rand::Rng;

pub struct Experimental {
    mother_spawn_interval: u64,
    mother_min_amount: usize,
    tick_count: u64,
}

impl Experimental {
    pub fn new() -> Self {
        Self {
            mother_spawn_interval: 100,
            mother_min_amount: 7,
            tick_count: 0,
        }
    }

    fn spawn_mother_cell(&self, world: &mut World) {
        if world.mother_cells.len() >= self.mother_min_amount {
            return;
        }

        // Try to find a valid position
        // Basic check: just random position within border
        let pos = world.border.random_position();
        
        // Ensure no overlap with existing large cells?
        // JS uses `gameServer.willCollide(pos, 149)`.
        // We can skip rigorous check for now or impl basic check.
        
        let id = world.next_id();
        let mother = MotherCell::new(id, pos, 0.0, 0); // Tick 0 for now
        world.add_mother_cell(mother);
    }
}

impl GameMode for Experimental {
    fn name(&self) -> &str {
        "Experimental"
    }

    fn id(&self) -> u32 {
        2
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
        self.tick_count += 1;

        // Spawn mother cells every mother_spawn_interval ticks (like JS version)
        // JS: if ((gameServer.tickCount % this.motherSpawnInterval) === 0) this.spawnMotherCell(gameServer);
        if self.tick_count % self.mother_spawn_interval == 0 {
            self.spawn_mother_cell(world);
        }

        // Update mother cells
        let mother_ids = world.mother_cells.clone();
        let food_count = world.food_cells.len(); // Capture before borrow
        
        for id in mother_ids {
            let mut actions = Vec::new(); // (Position, Size) to spawn food
            
            if let Some(CellEntry::Mother(mother)) = world.get_cell_mut(id) {
                 // Logic from JS:
                 // Update interval: random 25-50 ticks normally, but 2 ticks when size > minSize
                 // JS: let updateInt = Math.random() * (50 - 25) + 25;
                 // JS: if (this.mothercells[i]._size > this.mothercells[i].minSize) updateInt = 2;
                 // JS: if ((gameServer.tickCount % ~~updateInt) === 0) this.mothercells[i].onUpdate();
                 
                 let update_interval = if mother.data().size > mother.min_size {
                     2 // Fast spawning when bigger
                 } else {
                     37 // Average of 25-50 range
                 };
                 
                 if self.tick_count % update_interval == 0 {
                     // Check food limit
                     if food_count < 2000 {
                         // motherFoodSpawnRate defaults to 2 in JS
                         let spawn_rate = 2;
                         
                         for _ in 0..spawn_rate {
                             if mother.data().size <= mother.min_size {
                                 break;
                             }
                             
                             // Shrink mother cell by 100 radius
                             // JS: size1 = Math.sqrt(this.radius - 100);
                             let radius = mother.data().radius;
                             let new_radius = (radius - 100.0).max(mother.min_size * mother.min_size);
                             mother.data_mut().set_size(new_radius.sqrt());
                             
                             // Calculate spawn position
                             let mut rng = rand::rng();
                             let angle = rng.random_range(0.0..std::f32::consts::TAU);
                             let dist = mother.data().size; 
                             let pos_x = mother.data().position.x + dist * angle.sin();
                             let pos_y = mother.data().position.y + dist * angle.cos();
                             
                             actions.push(Vec2::new(pos_x, pos_y));
                         }
                     }
                 }
            }
            
            // Execute spawns
            for pos in actions {
                let id = world.next_id();
                // Random food size between min and max
                let mut rng = rand::rng();
                let size = 10.0 + rng.random::<f32>() * 10.0; // 10-20 range
                let mut food = Food::new(id, pos, size, 0);
                food.set_color(World::random_color());
                food.from_mother = true; // Mark as from mother cell
                
                // Apply boost
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let boost_dist = 32.0 + 42.0 * rng.random::<f32>();
                food.data_mut().set_boost(boost_dist, angle);
                
                world.add_food(food);
                world.add_moving(id);
            }
        }
    }
}
