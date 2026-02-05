//! Mother cell (experimental mode).

use super::cell::{Cell, CellData, CellType};
use super::virus::Virus;
use glam::Vec2;
use protocol::Color;

/// Default mother cell color (brownish-red).
pub const MOTHER_COLOR: Color = Color::new(206, 99, 99);

/// Mother cell that spawns food in experimental mode.
#[derive(Debug, Clone)]
pub struct MotherCell {
    data: CellData,
    /// Minimum size the mother cell can shrink to.
    pub min_size: f32,
}

impl MotherCell {
    /// Create a new mother cell.
    pub fn new(node_id: u32, position: Vec2, size: f32, tick: u64) -> Self {
        let min_size = 149.0; // Same as JS MotherCell.minSize
        let actual_size = if size > 0.0 { size } else { min_size };
        
        let mut data = CellData::new(node_id, CellType::MotherCell, position, actual_size, tick);
        data.spiked = true;
        data.color = MOTHER_COLOR;
        
        Self { data, min_size }
    }

    /// Convert to a regular virus for shared behavior.
    pub fn as_virus(&self) -> Virus {
        let mut virus = Virus::new(
            self.data.node_id,
            self.data.position,
            self.data.size,
            self.data.tick_of_birth,
        );
        virus.is_mother_cell = true;
        virus.set_color(MOTHER_COLOR);
        virus
    }
}

impl Cell for MotherCell {
    fn data(&self) -> &CellData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut CellData {
        &mut self.data
    }

    /// Mother cells can eat player cells, viruses, and ejected mass.
    fn can_eat(&self) -> bool {
        true
    }
}
