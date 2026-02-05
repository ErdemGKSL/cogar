//! Base cell type and common functionality.

use glam::Vec2;
use protocol::Color;

// Performance: Constants for cell calculations
const MASS_DIVISOR: f32 = 100.0;  // Mass = radius / 100

/// Cell type enum matching JS cellType values.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellType {
    /// Player cell (cellType = 0)
    #[default]
    Player = 0,
    /// Food pellet (cellType = 1)
    Food = 1,
    /// Virus (cellType = 2)
    Virus = 2,
    /// Ejected mass (cellType = 3)
    EjectedMass = 3,
    /// Mother cell (experimental mode)
    MotherCell = 4,
}

/// Common cell data shared by all cell types.
#[derive(Debug, Clone)]
pub struct CellData {
    /// Unique node ID (scrambled when sent to clients).
    pub node_id: u32,
    /// Owner client ID (None for food, viruses, etc.)
    pub owner_id: Option<u32>,
    /// Cell type.
    pub cell_type: CellType,
    /// Position in world coordinates.
    pub position: Vec2,
    /// Cell size (sqrt of mass * 100).
    pub size: f32,
    /// Cell radius (size^2, used for collision).
    pub radius: f32,
    /// Cell mass (radius / 100).
    pub mass: f32,
    /// Cell color.
    pub color: Color,
    /// Tick when the cell was born.
    pub tick_of_birth: u64,
    /// Whether the cell is marked as removed.
    pub is_removed: bool,
    /// Whether the cell is agitated (spiky animation).
    pub is_agitated: bool,
    /// Whether the cell has spikes (viruses).
    pub spiked: bool,
    /// Boost movement data.
    pub boost: Option<BoostData>,
    /// ID of the cell that killed this cell (for eat animation).
    pub killed_by: Option<u32>,
}

impl CellData {
    /// Create new cell data.
    pub fn new(node_id: u32, cell_type: CellType, position: Vec2, size: f32, tick: u64) -> Self {
        let radius = size * size;
        let mass = radius / MASS_DIVISOR;
        Self {
            node_id,
            owner_id: None,
            cell_type,
            position,
            size,
            radius,
            mass,
            color: Color::default(),
            tick_of_birth: tick,
            is_removed: false,
            is_agitated: false,
            spiked: false,
            boost: None,
            killed_by: None,
        }
    }

    /// Set the cell size and update radius/mass.
    // #[track_caller]
    #[inline]
    pub fn set_size(&mut self, size: f32) {
        self.size = size;
        self.radius = size * size;
        self.mass = self.radius / MASS_DIVISOR;
    }

    /// Get the cell's age in ticks.
    #[inline]
    pub fn get_age(&self, current_tick: u64) -> u64 {
        current_tick.saturating_sub(self.tick_of_birth)
    }

    /// Called when this cell eats another cell.
    #[track_caller]
    pub fn on_eat(&mut self, other_radius: f32) {
        // if self.cell_type == CellType::Player {
        //     let location = std::panic::Location::caller();
        //     let old_radius = self.radius;
        //     let old_mass = self.mass;
        //     tracing::debug!("on_eat for PLAYER cell {} called from {}:{}:{} | other_radius: {:.2}, old_radius: {:.2}, old_mass: {:.2}", 
        //         self.node_id, location.file(), location.line(), location.column(), other_radius, old_radius, old_mass);
        // }
        let new_radius = self.radius + other_radius;
        self.set_size(new_radius.sqrt());
    }

    /// Set boost for the cell (used for ejection, splitting).
    #[inline]
    pub fn set_boost(&mut self, distance: f32, angle: f32) {
        self.boost = Some(BoostData {
            distance,
            direction: Vec2::new(angle.sin(), angle.cos()),
            angle,
        });
    }

    /// Set boost with a pre-computed direction vector (used for splitting).
    #[inline]
    pub fn set_boost_direction(&mut self, distance: f32, direction: Vec2) {
        let angle = direction.y.atan2(direction.x);
        self.boost = Some(BoostData {
            distance,
            direction,
            angle,
        });
    }

    /// Check and clamp position to border.
    #[inline]
    pub fn check_border(&mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) {
        let half_size = self.size / 2.0;
        self.position.x = self.position.x.clamp(min_x + half_size, max_x - half_size);
        self.position.y = self.position.y.clamp(min_y + half_size, max_y - half_size);
    }

    /// Update boost movement (called each tick).
    /// Returns true if the cell is still boosting.
    /// Matches JS moveCell: speed = boostDistance / 10; boostDistance -= speed;
    pub fn update_boost(&mut self, border_min: Vec2, border_max: Vec2) -> bool {
        if let Some(ref mut boost) = self.boost {
            if boost.distance < 1.0 {
                boost.distance = 0.0;
                self.boost = None;
                return false;
            }

            // Exponential decay: move 1/10 of remaining distance each tick
            let move_dist = boost.distance / 10.0;
            boost.distance -= move_dist;
            self.position += boost.direction * move_dist;

            // Check border
            self.check_border(border_min.x, border_min.y, border_max.x, border_max.y);

            true
        } else {
            false
        }
    }
}

/// Boost movement data.
#[derive(Debug, Clone, Copy)]
pub struct BoostData {
    /// Remaining distance to travel.
    pub distance: f32,
    /// Direction vector (normalized).
    pub direction: Vec2,
    /// Original angle.
    pub angle: f32,
}

/// Trait for all cell types.
pub trait Cell: Send + Sync {
    /// Get the common cell data.
    fn data(&self) -> &CellData;

    /// Get mutable cell data.
    fn data_mut(&mut self) -> &mut CellData;

    /// Check if this cell can eat other cells.
    fn can_eat(&self) -> bool {
        false
    }

    /// Called when the cell is added to the world.
    fn on_add(&mut self) {}

    /// Called when the cell is removed from the world.
    fn on_remove(&mut self) {}

    /// Called when this cell is eaten by another.
    fn on_eaten(&mut self, _eater_id: u32) {}

    /// Get the cell's movement speed (for player cells).
    fn get_speed(&self, _distance: f32) -> f32 {
        0.0
    }
}
