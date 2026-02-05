//! World state management.
//!
//! Manages all cells in the game world.

use crate::entity::{Cell, CellData, CellType, EjectedMass, Food, PlayerCell, Virus, MotherCell};
use crate::spatial::{QuadItem, QuadTree};
use glam::Vec2;
use protocol::Color;
use rand::Rng;
use std::collections::HashMap;

/// The game world containing all cells.
#[derive(Debug)]
pub struct World {
    /// Next node ID to assign.
    next_node_id: u32,

    /// All cells by ID.
    pub(crate) cells: HashMap<u32, CellEntry>,

    /// Player cells (cellType = 0).
    pub player_cells: Vec<u32>,
    /// Food pellets (cellType = 1).
    pub food_cells: Vec<u32>,
    /// Viruses (cellType = 2).
    pub virus_cells: Vec<u32>,
    /// Ejected mass (cellType = 3).
    pub eject_cells: Vec<u32>,
    /// Mother cells (cellType = 4).
    pub mother_cells: Vec<u32>,

    /// Position tracking for O(1) removal
    player_pos: HashMap<u32, usize>,
    food_pos: HashMap<u32, usize>,
    virus_pos: HashMap<u32, usize>,
    eject_pos: HashMap<u32, usize>,
    mother_pos: HashMap<u32, usize>,
    moving_pos: HashMap<u32, usize>,

    /// Cells that are currently moving (boosted).
    pub moving_cells: Vec<u32>,

    /// World border.
    pub border: WorldBorder,

    /// QuadTree for spatial queries.
    pub quad_tree: QuadTree,
}

/// A cell entry in the world.
#[derive(Debug)]
pub enum CellEntry {
    Player(PlayerCell),
    Food(Food),
    Virus(Virus),
    Eject(EjectedMass),
    Mother(MotherCell),
}

impl CellEntry {
    /// Get the common cell data.
    pub fn data(&self) -> &CellData {
        match self {
            CellEntry::Player(c) => c.data(),
            CellEntry::Food(c) => c.data(),
            CellEntry::Virus(c) => c.data(),
            CellEntry::Eject(c) => c.data(),
            CellEntry::Mother(c) => c.data(),
        }
    }

    /// Get mutable cell data.
    pub fn data_mut(&mut self) -> &mut CellData {
        match self {
            CellEntry::Player(c) => c.data_mut(),
            CellEntry::Food(c) => c.data_mut(),
            CellEntry::Virus(c) => c.data_mut(),
            CellEntry::Eject(c) => c.data_mut(),
            CellEntry::Mother(c) => c.data_mut(),
        }
    }

    /// Check if this cell can eat.
    #[allow(dead_code)]
    pub fn can_eat(&self) -> bool {
        match self {
            CellEntry::Player(c) => c.can_eat(),
            CellEntry::Food(c) => c.can_eat(),
            CellEntry::Virus(c) => c.can_eat(),
            CellEntry::Eject(c) => c.can_eat(),
            CellEntry::Mother(c) => c.can_eat(),
        }
    }
}

/// World border bounds.
#[derive(Debug, Clone, Copy)]
pub struct WorldBorder {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub width: f32,
    pub height: f32,
}

impl WorldBorder {
    pub fn new(width: f32, height: f32) -> Self {
        let half_w = width / 2.0;
        let half_h = height / 2.0;
        Self {
            min_x: -half_w,
            min_y: -half_h,
            max_x: half_w,
            max_y: half_h,
            width,
            height,
        }
    }

    /// Get a random position within the border.
    #[inline]
    pub fn random_position(&self) -> Vec2 {
        let mut rng = rand::rng();
        Vec2::new(
            rng.random_range(self.min_x..self.max_x),
            rng.random_range(self.min_y..self.max_y),
        )
    }
}

impl World {
    /// Create a new world with the given border size.
    pub fn new(width: f32, height: f32) -> Self {
        let border = WorldBorder::new(width, height);
        Self {
            next_node_id: 1,
            cells: HashMap::with_capacity(2048),
            player_cells: Vec::with_capacity(256),
            food_cells: Vec::with_capacity(1024),
            virus_cells: Vec::with_capacity(64),
            eject_cells: Vec::with_capacity(256),
            mother_cells: Vec::with_capacity(16),
            player_pos: HashMap::with_capacity(256),
            food_pos: HashMap::with_capacity(1024),
            virus_pos: HashMap::with_capacity(64),
            eject_pos: HashMap::with_capacity(256),
            mother_pos: HashMap::with_capacity(16),
            moving_pos: HashMap::with_capacity(256),
            moving_cells: Vec::with_capacity(256),
            quad_tree: QuadTree::for_world(border.min_x, border.min_y, border.max_x, border.max_y),
            border,
        }
    }

    /// Get the next node ID.
    pub fn next_id(&mut self) -> u32 {
        let id = self.next_node_id;
        self.next_node_id = self.next_node_id.wrapping_add(1);
        if self.next_node_id == 0 {
            self.next_node_id = 1; // Skip 0
        }
        id
    }

    /// Get a cell by ID.
    #[inline]
    pub fn get_cell(&self, id: u32) -> Option<&CellEntry> {
        self.cells.get(&id)
    }

    /// Get a mutable cell by ID.
    #[inline]
    pub fn get_cell_mut(&mut self, id: u32) -> Option<&mut CellEntry> {
        self.cells.get_mut(&id)
    }

    /// Add a player cell to the world.
    pub fn add_player_cell(&mut self, cell: PlayerCell) -> u32 {
        let id = cell.data().node_id;
        let data = cell.data();
        self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        let pos = self.player_cells.len();
        self.player_cells.push(id);
        self.player_pos.insert(id, pos);
        self.cells.insert(id, CellEntry::Player(cell));
        id
    }

    /// Add a food cell to the world.
    pub fn add_food(&mut self, cell: Food) -> u32 {
        let id = cell.data().node_id;
        let data = cell.data();
        self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        let pos = self.food_cells.len();
        self.food_cells.push(id);
        self.food_pos.insert(id, pos);
        self.cells.insert(id, CellEntry::Food(cell));
        id
    }

    /// Add a virus to the world.
    pub fn add_virus(&mut self, cell: Virus) -> u32 {
        let id = cell.data().node_id;
        let data = cell.data();
        self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        let pos = self.virus_cells.len();
        self.virus_cells.push(id);
        self.virus_pos.insert(id, pos);
        self.cells.insert(id, CellEntry::Virus(cell));
        id
    }

    /// Add ejected mass to the world.
    pub fn add_eject(&mut self, cell: EjectedMass) -> u32 {
        let id = cell.data().node_id;
        let data = cell.data();
        self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        let pos = self.eject_cells.len();
        self.eject_cells.push(id);
        self.eject_pos.insert(id, pos);
        self.cells.insert(id, CellEntry::Eject(cell));
        id
    }

    /// Add a mother cell to the world.
    pub fn add_mother_cell(&mut self, cell: MotherCell) -> u32 {
        let id = cell.data().node_id;
        let data = cell.data();
        self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        let pos = self.mother_cells.len();
        self.mother_cells.push(id);
        self.mother_pos.insert(id, pos);
        self.cells.insert(id, CellEntry::Mother(cell));
        id
    }

    /// Add to moving cells list
    pub fn add_moving(&mut self, id: u32) {
        if !self.moving_pos.contains_key(&id) {
            let pos = self.moving_cells.len();
            self.moving_cells.push(id);
            self.moving_pos.insert(id, pos);
        }
    }

    /// Remove from moving cells list (O(1))
    pub fn remove_from_moving(&mut self, id: u32) {
        if let Some(pos) = self.moving_pos.remove(&id) {
            let last_pos = self.moving_cells.len() - 1;
            if pos != last_pos {
                let swapped_id = self.moving_cells[last_pos];
                self.moving_cells.swap(pos, last_pos);
                self.moving_pos.insert(swapped_id, pos);
            }
            self.moving_cells.pop();
        }
    }

    /// Remove a cell from the world (O(1) for type lists).
    pub fn remove_cell(&mut self, id: u32) -> Option<CellEntry> {
        if let Some(entry) = self.cells.remove(&id) {
            // Remove from QuadTree
            self.quad_tree.remove(id);

            // Remove from type-specific list using O(1) swap_remove
            match entry.data().cell_type {
                CellType::Player => {
                    if let Some(pos) = self.player_pos.remove(&id) {
                        let last_pos = self.player_cells.len() - 1;
                        if pos != last_pos {
                            let swapped_id = self.player_cells[last_pos];
                            self.player_cells.swap(pos, last_pos);
                            self.player_pos.insert(swapped_id, pos);
                        }
                        self.player_cells.pop();
                    }
                }
                CellType::Food => {
                    if let Some(pos) = self.food_pos.remove(&id) {
                        let last_pos = self.food_cells.len() - 1;
                        if pos != last_pos {
                            let swapped_id = self.food_cells[last_pos];
                            self.food_cells.swap(pos, last_pos);
                            self.food_pos.insert(swapped_id, pos);
                        }
                        self.food_cells.pop();
                    }
                }
                CellType::Virus => {
                    if let Some(pos) = self.virus_pos.remove(&id) {
                        let last_pos = self.virus_cells.len() - 1;
                        if pos != last_pos {
                            let swapped_id = self.virus_cells[last_pos];
                            self.virus_cells.swap(pos, last_pos);
                            self.virus_pos.insert(swapped_id, pos);
                        }
                        self.virus_cells.pop();
                    }
                }
                CellType::EjectedMass => {
                    if let Some(pos) = self.eject_pos.remove(&id) {
                        let last_pos = self.eject_cells.len() - 1;
                        if pos != last_pos {
                            let swapped_id = self.eject_cells[last_pos];
                            self.eject_cells.swap(pos, last_pos);
                            self.eject_pos.insert(swapped_id, pos);
                        }
                        self.eject_cells.pop();
                    }
                }
                CellType::MotherCell => {
                    if let Some(pos) = self.mother_pos.remove(&id) {
                        let last_pos = self.mother_cells.len() - 1;
                        if pos != last_pos {
                            let swapped_id = self.mother_cells[last_pos];
                            self.mother_cells.swap(pos, last_pos);
                            self.mother_pos.insert(swapped_id, pos);
                        }
                        self.mother_cells.pop();
                    }
                }
            }

            // Remove from moving list (O(1))
            self.remove_from_moving(id);

            Some(entry)
        } else {
            None
        }
    }

    /// Get the count of each cell type.
    #[inline]
    pub fn cell_counts(&self) -> CellCounts {
        CellCounts {
            players: self.player_cells.len(),
            food: self.food_cells.len(),
            viruses: self.virus_cells.len(),
            ejected: self.eject_cells.len(),
            total: self.cells.len(),
        }
    }

    /// Generate a random color.
    #[inline]
    pub fn random_color() -> Color {
        let mut rng = rand::rng();
        Color::new(
            rng.random_range(50..=255),
            rng.random_range(50..=255),
            rng.random_range(50..=255),
        )
    }

    /// Spawn food up to the minimum amount.
    #[inline]
    pub fn spawn_food(&mut self, min_amount: usize, max_amount: usize, spawn_amount: usize, min_size: f32, max_size: f32, tick: u64) {
        let current = self.food_cells.len();
        if current >= max_amount {
            return;
        }

        let to_spawn = spawn_amount.min(max_amount - current);
        let need_to_reach_min = current < min_amount;

        let count = if need_to_reach_min {
            (min_amount - current).min(to_spawn * 2) // Spawn faster to reach min
        } else {
            to_spawn
        };

        let mut rng = rand::rng();
        for _ in 0..count {
            let pos = self.border.random_position();
            let size = if max_size > min_size {
                rng.random_range(min_size..max_size)
            } else {
                min_size
            };
            let id = self.next_id();
            let mut food = Food::new(id, pos, size, tick);
            food.set_color(Self::random_color());
            self.add_food(food);
        }
    }

    /// Spawn viruses up to the minimum amount.
    pub fn spawn_viruses(&mut self, min_amount: usize, max_amount: usize, min_size: f32, tick: u64) {
        let current = self.virus_cells.len();
        if current >= min_amount {
            return;
        }

        let to_spawn = min_amount - current;
        for _ in 0..to_spawn {
            let pos = self.border.random_position();
            let id = self.next_id();
            let virus = Virus::new(id, pos, min_size, tick);
            self.add_virus(virus);

            if self.virus_cells.len() >= max_amount {
                break;
            }
        }
    }

    /// Iterate over all cells.
    #[inline]
    pub fn iter_cells(&self) -> impl Iterator<Item = (&u32, &CellEntry)> {
        self.cells.iter()
    }

    /// Iterate over all cells mutably.
    #[inline]
    pub fn iter_cells_mut(&mut self) -> impl Iterator<Item = (&u32, &mut CellEntry)> {
        self.cells.iter_mut()
    }

    /// Find all cells within a radius of a point using the QuadTree.
    #[inline]
    pub fn find_cells_in_radius(&mut self, cx: f32, cy: f32, radius: f32) -> Vec<u32> {
        self.quad_tree.find_in_radius(cx, cy, radius)
    }

    /// Update a cell's position in the QuadTree.
    #[inline]
    pub fn update_cell_position(&mut self, id: u32) {
        if let Some(cell) = self.cells.get(&id) {
            let data = cell.data();
            self.quad_tree.update(id, data.position.x, data.position.y, data.size);
        }
    }

    /// Rebuild the entire QuadTree (use after bulk updates).
    #[inline]
    pub fn rebuild_quadtree(&mut self) {
        self.quad_tree.clear();
        for (&id, cell) in &self.cells {
            let data = cell.data();
            self.quad_tree.insert(QuadItem::new(id, data.position.x, data.position.y, data.size));
        }
    }
}

/// Cell count statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellCounts {
    pub players: usize,
    pub food: usize,
    pub viruses: usize,
    pub ejected: usize,
    pub total: usize,
}
