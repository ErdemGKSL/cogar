#![allow(dead_code)]
//! QuadTree for spatial indexing.
//!
//! This mirrors the QuadNode.js implementation from MultiOgar-Edited.

use std::collections::HashMap;

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Bounds {
    pub fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    /// Create bounds from center and size.
    #[inline]
    pub fn from_center(cx: f32, cy: f32, size: f32) -> Self {
        Self {
            min_x: cx - size,
            min_y: cy - size,
            max_x: cx + size,
            max_y: cy + size,
        }
    }

    /// Check if two bounds intersect.
    #[inline]
    pub fn intersects(&self, other: &Bounds) -> bool {
        !(other.min_x >= self.max_x
            || other.max_x <= self.min_x
            || other.min_y >= self.max_y
            || other.max_y <= self.min_y)
    }

    /// Get the width of the bounds.
    #[inline]
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Get the height of the bounds.
    #[inline]
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Get center X.
    #[inline]
    pub fn center_x(&self) -> f32 {
        (self.min_x + self.max_x) / 2.0
    }

    /// Get center Y.
    #[inline]
    pub fn center_y(&self) -> f32 {
        (self.min_y + self.max_y) / 2.0
    }
}

/// An item stored in the QuadTree.
#[derive(Debug, Clone)]
pub struct QuadItem {
    /// Unique node ID.
    pub id: u32,
    /// Current position X.
    pub x: f32,
    /// Current position Y.
    pub y: f32,
    /// Current size (radius).
    pub size: f32,
    /// Bounding box (cached).
    pub bound: Bounds,
}

impl QuadItem {
    #[inline]
    pub fn new(id: u32, x: f32, y: f32, size: f32) -> Self {
        Self {
            id,
            x,
            y,
            size,
            bound: Bounds::from_center(x, y, size),
        }
    }

    /// Update position and size, recalculating bounds.
    #[inline]
    pub fn update(&mut self, x: f32, y: f32, size: f32) {
        self.x = x;
        self.y = y;
        self.size = size;
        self.bound = Bounds::from_center(x, y, size);
    }
}

/// QuadTree for efficient spatial queries.
///
/// Uses a simple flat storage with lazy rebuild for optimal performance.
/// Items are stored in a Vec with HashMap index for O(1) lookup.
/// Tree is rebuilt only when needed (before queries if dirty).
pub struct QuadTree {
    /// All items indexed by position in this vec.
    items: Vec<QuadItem>,
    /// Map from item ID to index in items vec.
    id_to_index: HashMap<u32, usize>,
    /// World bounds.
    bounds: Bounds,
    /// Whether the tree needs rebuilding.
    dirty: bool,
    /// Grid cells for spatial hashing (faster than tree for uniform distribution).
    grid: Vec<Vec<u32>>,
    /// Grid dimensions.
    grid_size: usize,
    /// Cell size for grid.
    cell_size: f32,
    /// Reusable seen bitset for collision detection (avoids HashSet allocation).
    seen_bits: Vec<u64>,
}

impl QuadTree {
    /// Create a new QuadTree with the given bounds.
    pub fn new(bound: Bounds, _max_children: usize, _max_level: u32) -> Self {
        // Use spatial hashing grid instead of tree - much faster for games
        let grid_size = 32; // 32x32 grid
        let cell_size = (bound.max_x - bound.min_x) / grid_size as f32;
        let grid = vec![Vec::with_capacity(16); grid_size * grid_size];
        // Allocate bitset for 65536 IDs (1024 u64s = 64KB)
        let seen_bits = vec![0u64; 1024];

        Self {
            items: Vec::with_capacity(1024),
            id_to_index: HashMap::with_capacity(1024),
            bounds: bound,
            dirty: false,
            grid,
            grid_size,
            cell_size,
            seen_bits,
        }
    }

    /// Create a QuadTree for the game world.
    pub fn for_world(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        Self::new(Bounds::new(min_x, min_y, max_x, max_y), 64, 8)
    }

    /// Get grid cell index for a position.
    #[inline]
    fn grid_index(&self, x: f32, y: f32) -> usize {
        let gx = ((x - self.bounds.min_x) / self.cell_size) as usize;
        let gy = ((y - self.bounds.min_y) / self.cell_size) as usize;
        let gx = gx.min(self.grid_size - 1);
        let gy = gy.min(self.grid_size - 1);
        gy * self.grid_size + gx
    }

    /// Insert or update an item in the tree.
    #[inline]
    pub fn insert(&mut self, item: QuadItem) {
        let id = item.id;

        if let Some(&idx) = self.id_to_index.get(&id) {
            // Update existing item
            self.items[idx] = item;
        } else {
            // Insert new item
            let idx = self.items.len();
            self.items.push(item);
            self.id_to_index.insert(id, idx);
        }
        self.dirty = true;
    }

    /// Remove an item from the tree.
    #[inline]
    pub fn remove(&mut self, id: u32) {
        if let Some(idx) = self.id_to_index.remove(&id) {
            // Swap-remove for efficiency
            self.items.swap_remove(idx);

            // Update the index of the swapped item
            if idx < self.items.len() {
                let swapped_id = self.items[idx].id;
                self.id_to_index.insert(swapped_id, idx);
            }
            self.dirty = true;
        }
    }

    /// Update an item's position and size.
    #[inline]
    pub fn update(&mut self, id: u32, x: f32, y: f32, size: f32) {
        if let Some(&idx) = self.id_to_index.get(&id) {
            self.items[idx].update(x, y, size);
            self.dirty = true;
        }
    }

    /// Rebuild the spatial hash grid.
    #[inline(never)] // Don't inline to keep instruction cache efficient
    fn rebuild_grid(&mut self) {
        if !self.dirty {
            return;
        }

        // Clear grid - faster than calling clear() on each vec
        for cell in &mut self.grid {
            cell.clear();
        }

        // Insert all items into grid cells they overlap
        // Pre-compute grid size to avoid repeated bounds checks
        let grid_size_minus_1 = self.grid_size - 1;
        let cell_size = self.cell_size;
        let bounds_min_x = self.bounds.min_x;
        let bounds_min_y = self.bounds.min_y;
        
        for item in &self.items {
            // Calculate which grid cells this item overlaps
            let min_gx = ((item.bound.min_x - bounds_min_x) / cell_size) as i32;
            let max_gx = ((item.bound.max_x - bounds_min_x) / cell_size) as i32;
            let min_gy = ((item.bound.min_y - bounds_min_y) / cell_size) as i32;
            let max_gy = ((item.bound.max_y - bounds_min_y) / cell_size) as i32;

            let min_gx = (min_gx.max(0) as usize).min(grid_size_minus_1);
            let max_gx = (max_gx as usize).min(grid_size_minus_1);
            let min_gy = (min_gy.max(0) as usize).min(grid_size_minus_1);
            let max_gy = (max_gy as usize).min(grid_size_minus_1);

            // Unroll small loops for better performance
            if max_gy - min_gy <= 2 && max_gx - min_gx <= 2 {
                // Common case: item spans 1-3 cells in each direction
                for gy in min_gy..=max_gy {
                    for gx in min_gx..=max_gx {
                        let grid_idx = gy * self.grid_size + gx;
                        unsafe { self.grid.get_unchecked_mut(grid_idx).push(item.id) };
                    }
                }
            } else {
                // Rare case: large item spans many cells
                for gy in min_gy..=max_gy {
                    let row_start = gy * self.grid_size;
                    for gx in min_gx..=max_gx {
                        self.grid[row_start + gx].push(item.id);
                    }
                }
            }
        }

        self.dirty = false;
    }

    /// Find all items whose bounds intersect with the given bounds.
    #[inline]
    pub fn find_in_bounds(&mut self, bound: &Bounds) -> Vec<u32> {
        self.rebuild_grid();

        // Calculate which grid cells to check
        let min_gx = ((bound.min_x - self.bounds.min_x) / self.cell_size) as i32;
        let max_gx = ((bound.max_x - self.bounds.min_x) / self.cell_size) as i32;
        let min_gy = ((bound.min_y - self.bounds.min_y) / self.cell_size) as i32;
        let max_gy = ((bound.max_y - self.bounds.min_y) / self.cell_size) as i32;

        let min_gx = min_gx.max(0) as usize;
        let max_gx = (max_gx as usize).min(self.grid_size - 1);
        let min_gy = min_gy.max(0) as usize;
        let max_gy = (max_gy as usize).min(self.grid_size - 1);

        // Pre-allocate for typical result size
        let mut result = Vec::with_capacity(64);
        
        // Clear seen bits for IDs we might encounter
        for bits in &mut self.seen_bits {
            *bits = 0;
        }

        for gy in min_gy..=max_gy {
            for gx in min_gx..=max_gx {
                let grid_idx = gy * self.grid_size + gx;
                // Direct slice access is faster than iterator
                let cell = unsafe { self.grid.get_unchecked(grid_idx) };
                for &id in cell {
                    // Bit-packing: check if we've seen this ID
                    let bit_idx = (id & 0xFFFF) as usize; // Support up to 65536 IDs
                    let word_idx = bit_idx >> 6; // Divide by 64
                    let bit_pos = bit_idx & 63; // Modulo 64
                    let mask = 1u64 << bit_pos;
                    
                    if word_idx < self.seen_bits.len() {
                        let seen_word = unsafe { self.seen_bits.get_unchecked_mut(word_idx) };
                        if (*seen_word & mask) == 0 {
                            *seen_word |= mask;
                            // Check actual intersection using O(1) lookup
                            if let Some(&idx) = self.id_to_index.get(&id) {
                                let item = unsafe { self.items.get_unchecked(idx) };
                                if item.bound.intersects(bound) {
                                    result.push(id);
                                }
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Find all items whose bounds intersect with a circle.
    #[inline]
    pub fn find_in_radius(&mut self, cx: f32, cy: f32, radius: f32) -> Vec<u32> {
        let bound = Bounds::from_center(cx, cy, radius);
        self.find_in_bounds(&bound)
    }

    /// Get an item by ID.
    #[inline]
    pub fn get(&self, id: u32) -> Option<&QuadItem> {
        self.id_to_index.get(&id).map(|&idx| &self.items[idx])
    }

    /// Get all items.
    #[inline]
    pub fn all_items(&self) -> &[QuadItem] {
        &self.items
    }

    /// Get the number of items.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.items.clear();
        self.id_to_index.clear();
        for cell in &mut self.grid {
            cell.clear();
        }
        // Clear seen bits
        for bits in &mut self.seen_bits {
            *bits = 0;
        }
        self.dirty = false;
    }
}

impl std::fmt::Debug for QuadTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuadTree")
            .field("items", &self.items.len())
            .field("bounds", &self.bounds)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds_intersects() {
        let a = Bounds::new(0.0, 0.0, 10.0, 10.0);
        let b = Bounds::new(5.0, 5.0, 15.0, 15.0);
        let c = Bounds::new(20.0, 20.0, 30.0, 30.0);

        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
        assert!(!a.intersects(&c));
        assert!(!c.intersects(&a));
    }

    #[test]
    fn test_quadtree_insert_find() {
        let mut tree = QuadTree::for_world(-100.0, -100.0, 100.0, 100.0);

        tree.insert(QuadItem::new(1, 0.0, 0.0, 10.0));
        tree.insert(QuadItem::new(2, 50.0, 50.0, 10.0));
        tree.insert(QuadItem::new(3, -50.0, -50.0, 10.0));

        assert_eq!(tree.len(), 3);

        // Find near origin
        let found = tree.find_in_radius(0.0, 0.0, 20.0);
        assert!(found.contains(&1));
        assert!(!found.contains(&2));
        assert!(!found.contains(&3));

        // Find near (50, 50)
        let found = tree.find_in_radius(50.0, 50.0, 20.0);
        assert!(!found.contains(&1));
        assert!(found.contains(&2));
        assert!(!found.contains(&3));
    }
}
