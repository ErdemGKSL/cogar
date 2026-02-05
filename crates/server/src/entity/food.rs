//! Food pellet cell.

use super::cell::{Cell, CellData, CellType};
use glam::Vec2;
use protocol::Color;

/// A food pellet that can be eaten by players.
#[derive(Debug, Clone)]
pub struct Food {
    data: CellData,
    /// Whether this food was spawned by a mother cell.
    pub from_mother: bool,
}

impl Food {
    /// Create a new food pellet.
    pub fn new(node_id: u32, position: Vec2, size: f32, tick: u64) -> Self {
        let mut data = CellData::new(node_id, CellType::Food, position, size, tick);
        data.spiked = false;
        Self {
            data,
            from_mother: false,
        }
    }

    /// Set the food color.
    pub fn set_color(&mut self, color: Color) {
        self.data.color = color;
    }
}

impl Cell for Food {
    fn data(&self) -> &CellData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut CellData {
        &mut self.data
    }

    fn can_eat(&self) -> bool {
        false
    }
}
