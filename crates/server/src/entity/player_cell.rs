//! Player cell.

use super::cell::{Cell, CellData, CellType};
use glam::Vec2;

/// A cell controlled by a player.
#[derive(Debug, Clone)]
pub struct PlayerCell {
    /// Cell data (public for direct access).
    pub cell_data: CellData,
    /// Whether this cell can remerge with siblings.
    pub can_remerge: bool,
    /// Tick when merge becomes possible (0 = immediately).
    pub merge_tick: u64,
}

impl PlayerCell {
    /// Create a new player cell.
    pub fn new(node_id: u32, owner_id: u32, position: Vec2, size: f32, tick: u64) -> Self {
        let mut data = CellData::new(node_id, CellType::Player, position, size, tick);
        data.owner_id = Some(owner_id);
        Self {
            cell_data: data,
            can_remerge: false,
            merge_tick: 0, // Will be set when cell splits
        }
    }

    /// Update merge status based on current tick and merge time config.
    /// Returns true if the cell can now remerge.
    pub fn update_merge(&mut self, current_tick: u64, merge_time_base: f32) -> bool {
        let age = current_tick.saturating_sub(self.cell_data.tick_of_birth);

        // Can't remerge if too young (splitRestoreTicks = 13)
        if age < 13 {
            self.can_remerge = false;
            return false;
        }

        // If no merge time configured, check if done boosting
        if merge_time_base <= 0.0 {
            self.can_remerge = self.cell_data.boost.map(|b| b.distance < 100.0).unwrap_or(true);
            return self.can_remerge;
        }

        // Calculate merge time based on cell size
        // JS: time = Math.max(playerMergeTime, cell._size * 0.2) * 25
        let time = (merge_time_base.max(self.cell_data.size * 0.2) * 25.0) as u64;
        self.can_remerge = age >= time;
        self.can_remerge
    }

    /// Calculate movement speed based on cell size.
    /// Formula from JS: 2.2 * Math.pow(size, -0.439) * 40 * (playerSpeed / 30)
    pub fn calculate_speed(&self, player_speed: f32) -> f32 {
        let base_speed = 2.2 * self.cell_data.size.powf(-0.439) * 40.0;
        base_speed * (player_speed / 30.0)
    }
}

impl Cell for PlayerCell {
    fn data(&self) -> &CellData {
        &self.cell_data
    }

    fn data_mut(&mut self) -> &mut CellData {
        &mut self.cell_data
    }

    fn can_eat(&self) -> bool {
        true
    }

    fn get_speed(&self, distance: f32) -> f32 {
        // Default speed factor (will be multiplied by config)
        let speed = self.calculate_speed(30.0);
        speed * (distance.min(32.0) / 32.0)
    }
}

