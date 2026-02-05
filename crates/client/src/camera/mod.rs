// Camera system - viewport, zoom, smooth follow
//
// Lerp behaviour is intentionally frame-rate-dependent to match the JS client:
//   position: camera.x = (camera.x + target.x) / 2          (50 % per frame, alive)
//             camera.x += (target.x - camera.x) / 20        (5 %  per frame, spectating)
//   zoom:     camera.scale += (target.scale - camera.scale) / 9
//
// Zoom formula (JS): Math.pow(Math.min(64 / totalSize, 1), 0.4)
use glam::Vec2;

pub struct Camera {
    pub position: Vec2,
    pub target_position: Vec2,
    pub zoom: f32,
    pub target_zoom: f32,
    pub zoom_factor: f32,
    pub size_scale: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            zoom: 1.0,
            target_zoom: 1.0,
            zoom_factor: 1.0,
            size_scale: 1.0,
        }
    }

    /// Called once per animation frame. `has_cells` controls position-lerp speed.
    /// Matches JS client behavior (frame-rate dependent smoothing).
    pub fn update(&mut self, has_cells: bool) {
        if has_cells {
            // 50 % lerp per frame (alive)
            self.position = (self.position + self.target_position) * 0.5;
        } else {
            // ~5 % lerp per frame (spectating / dead)
            self.position += (self.target_position - self.position) / 20.0;
        }
        // Scale always lerps at 1/9 per frame
        self.zoom += (self.target_zoom - self.zoom) / 9.0;
    }

    /// Set camera targets from the player's interpolated cell positions and sizes.
    /// Must be called every frame before `update`.
    pub fn follow_cells(&mut self, cell_positions: &[Vec2], cell_sizes: &[f32]) {
        if cell_positions.is_empty() {
            return;
        }

        // Target = average position of owned cells
        let sum: Vec2 = cell_positions.iter().copied().sum();
        self.target_position = sum / cell_positions.len() as f32;

        // JS: sizeScale = Math.pow(Math.min(64 / totalSize, 1), 0.4)
        let total_size: f32 = cell_sizes.iter().sum();
        let base_zoom = (64.0_f32 / total_size).min(1.0).powf(0.4);
        self.size_scale = base_zoom;
        self.target_zoom = base_zoom * self.zoom_factor;
    }

    /// Adjust manual zoom factor (mouse wheel). Clamped to a safe range.
    pub fn adjust_zoom_factor(&mut self, delta: f32) {
        let next = self.zoom_factor * delta;
        self.zoom_factor = next.clamp(0.25, 2.5);
        // Keep target zoom consistent with current zoom (used when spectating)
        self.target_zoom = self.target_zoom.clamp(0.05, 5.0);
    }

    /// Apply a new base zoom (e.g. spectator update), respecting zoom factor.
    pub fn set_base_zoom(&mut self, base_zoom: f32) {
        self.size_scale = base_zoom;
        self.target_zoom = base_zoom * self.zoom_factor;
    }

    /// Convert screen coordinates to world coordinates.
    #[inline]
    pub fn screen_to_world(&self, screen_pos: Vec2, screen_center: Vec2) -> Vec2 {
        (screen_pos - screen_center) / self.zoom + self.position
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}
