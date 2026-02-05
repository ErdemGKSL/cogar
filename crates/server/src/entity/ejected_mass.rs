//! Ejected mass cell.

use super::cell::{Cell, CellData, CellType};
use glam::Vec2;
use protocol::Color;

/// Mass ejected by a player (W key).
#[derive(Debug, Clone)]
pub struct EjectedMass {
    data: CellData,
}

impl EjectedMass {
    /// Create new ejected mass.
    pub fn new(node_id: u32, position: Vec2, size: f32, tick: u64) -> Self {
        let mut data = CellData::new(node_id, CellType::EjectedMass, position, size, tick);
        data.spiked = false;
        Self { data }
    }

    /// Set the color (usually inherits from ejecting player).
    pub fn set_color(&mut self, color: Color) {
        self.data.color = color;
    }
}

impl Cell for EjectedMass {
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
