//! Virus cell.

use super::cell::{Cell, CellData, CellType};
use glam::Vec2;
use protocol::Color;

/// Default virus color (green).
pub const VIRUS_COLOR: Color = Color::new(51, 255, 51);

/// A virus that can pop player cells.
#[derive(Debug, Clone)]
pub struct Virus {
    data: CellData,
    /// Whether this is a mother cell (experimental mode).
    pub is_mother_cell: bool,
}

impl Virus {
    /// Create a new virus.
    pub fn new(node_id: u32, position: Vec2, size: f32, tick: u64) -> Self {
        let mut data = CellData::new(node_id, CellType::Virus, position, size, tick);
        data.spiked = true;
        data.color = VIRUS_COLOR;
        Self {
            data,
            is_mother_cell: false,
        }
    }

    /// Set a random color for the virus.
    pub fn set_color(&mut self, color: Color) {
        self.data.color = color;
    }
}

impl Cell for Virus {
    fn data(&self) -> &CellData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut CellData {
        &mut self.data
    }

    /// Viruses can eat ejected mass (to grow/split).
    fn can_eat(&self) -> bool {
        true
    }
}
