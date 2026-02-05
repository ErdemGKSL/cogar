//! Collision detection and resolution.
//!
//! This module handles cell-cell collisions including:
//! - Eating logic (when one cell consumes another)
//! - Rigid body collisions (bouncing between same-owner cells)
//! - Virus popping logic

use glam::Vec2;

// Performance: Compile-time constants for collision/eating logic
pub const PLAYER_EAT_MULT: f32 = 1.15;  // Player must be 15% larger to eat
pub const MOTHER_EAT_MULT: f32 = 1.0;   // Mother cells can eat equal size
pub const MASS_CONVERSION: f32 = 100.0; // Mass = size² / 100

/// Result of checking collision between two cells.
#[derive(Debug)]
pub struct CollisionResult {
    /// First cell ID
    pub cell_id: u32,
    /// Second cell ID
    pub check_id: u32,
    /// Combined radius of both cells
    pub r: f32,
    /// Distance X component
    pub dx: f32,
    /// Distance Y component
    pub dy: f32,
    /// Actual distance
    pub d: f32,
    /// Squared distance
    pub squared: f32,
    /// Push amount for rigid collisions
    pub push: f32,
}

impl CollisionResult {
    /// Check if cells are actually colliding.
    pub fn is_colliding(&self) -> bool {
        self.d < self.r
    }
}

/// Check collision between two cells.
/// Returns collision data if they could potentially interact.
#[inline]
pub fn check_cell_collision(
    cell_pos: Vec2,
    cell_size: f32,
    check_pos: Vec2,
    check_size: f32,
    cell_id: u32,
    check_id: u32,
) -> CollisionResult {
    let r = cell_size + check_size;
    let dx = check_pos.x - cell_pos.x;
    let dy = check_pos.y - cell_pos.y;
    let squared = dx * dx + dy * dy;
    let sqrt = squared.sqrt();

    let push = if sqrt > 0.0 {
        ((r - sqrt) / sqrt).min(r - sqrt)
    } else {
        0.0
    };

    CollisionResult {
        cell_id,
        check_id,
        r,
        dx,
        dy,
        d: sqrt,
        squared,
        push,
    }
}

/// Calculate mass from size (squared).
/// Matches JS: Math.pow(size, 2) / 100
#[inline]
pub fn size_to_mass(size: f32) -> f32 {
    (size * size) / MASS_CONVERSION
}

/// Calculate size from mass.
/// Matches JS: Math.sqrt(100 * mass)
#[inline]
pub fn mass_to_size(mass: f32) -> f32 {
    (MASS_CONVERSION * mass).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_mass_conversion() {
        let mass = 100.0;
        let size = mass_to_size(mass);
        let back = size_to_mass(size);
        assert!((back - mass).abs() < 0.001);
    }

    #[test]
    fn test_collision_check() {
        let result = check_cell_collision(
            Vec2::new(0.0, 0.0),
            50.0,
            Vec2::new(30.0, 0.0),
            20.0,
            1,
            2,
        );

        assert!(result.is_colliding()); // 50 + 20 = 70, distance = 30
        assert_eq!(result.d, 30.0);
    }

    #[test]
    fn test_no_collision() {
        let result = check_cell_collision(
            Vec2::new(0.0, 0.0),
            10.0,
            Vec2::new(100.0, 0.0),
            10.0,
            1,
            2,
        );

        assert!(!result.is_colliding()); // 10 + 10 = 20, distance = 100
    }
}
