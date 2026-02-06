// Canvas rendering - grid, cells, skins, UI overlays
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};
use glam::Vec2;
use crate::game::Cell;
use crate::utils;
use std::collections::HashSet;
use std::f32::consts::PI;
use std::f64::consts::TAU;
use std::cell::RefCell;

pub struct Renderer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    // Offscreen canvases for caching static elements
    grid_cache: RefCell<Option<(HtmlCanvasElement, f32, f32, f32, bool)>>, // (canvas, zoom, cam_x, cam_y, dark_theme)
    bg_cache: RefCell<Option<(HtmlCanvasElement, f32, f32, f32, bool)>>, // (canvas, zoom, cam_x, cam_y, dark_theme)
}

impl Renderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let ctx = canvas
            .get_context("2d")?
            .ok_or("Failed to get 2d context")?
            .dyn_into::<CanvasRenderingContext2d>()?;
        
        Ok(Self {
            canvas,
            ctx,
            grid_cache: RefCell::new(None),
            bg_cache: RefCell::new(None),
        })
    }

    #[inline(always)]
    pub fn width(&self) -> f32 {
        self.canvas.width() as f32
    }

    #[inline(always)]
    pub fn height(&self) -> f32 {
        self.canvas.height() as f32
    }

    #[inline]
    pub fn clear(&self, background: &str) {
        self.ctx.set_fill_style_str(background);
        self.ctx.fill_rect(0.0, 0.0, self.width() as f64, self.height() as f64);
    }

    #[inline]
    pub fn draw_grid(&self, border: (f32, f32, f32, f32), camera_pos: Vec2, zoom: f32, dark_theme: bool) {
        // Check if we can use cached grid
        if let Some((cached_canvas, cached_zoom, cached_x, cached_y, cached_theme)) = self.grid_cache.borrow().as_ref() {
            // Cache is valid if zoom and camera position haven't changed significantly
            let zoom_match = (cached_zoom - zoom).abs() < 0.001;
            let pos_match = (cached_x - camera_pos.x).abs() < 1.0 && (cached_y - camera_pos.y).abs() < 1.0;
            let theme_match = *cached_theme == dark_theme;
            
            if zoom_match && pos_match && theme_match {
                // Use cached grid - just blit it to the main canvas
                let _ = self.ctx.draw_image_with_html_canvas_element(cached_canvas, 0.0, 0.0);
                return;
            }
        }

        // Need to render grid - create or reuse offscreen canvas
        let cache_canvas = if let Some((canvas, _, _, _, _)) = self.grid_cache.borrow().as_ref() {
            canvas.clone()
        } else {
            let document = web_sys::window().unwrap().document().unwrap();
            let canvas = document.create_element("canvas").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
            canvas
        };

        cache_canvas.set_width(self.canvas.width());
        cache_canvas.set_height(self.canvas.height());

        let cache_ctx = cache_canvas
            .get_context("2d").unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>().unwrap();

        // Render grid to cache canvas
        self.render_grid_to_context(&cache_ctx, self.width(), self.height(), border, camera_pos, zoom, dark_theme);

        // Blit cache to main canvas
        let _ = self.ctx.draw_image_with_html_canvas_element(&cache_canvas, 0.0, 0.0);

        // Update cache
        *self.grid_cache.borrow_mut() = Some((cache_canvas, zoom, camera_pos.x, camera_pos.y, dark_theme));
    }

    #[inline]
    fn render_grid_to_context(
        &self,
        ctx: &CanvasRenderingContext2d,
        width: f32,
        height: f32,
        border: (f32, f32, f32, f32),
        camera_pos: Vec2,
        zoom: f32,
        dark_theme: bool
    ) {
        let (min_x, min_y, max_x, max_y) = border;
        let world_w = max_x - min_x;
        let world_h = max_y - min_y;
        if world_w <= 0.0 || world_h <= 0.0 {
            return;
        }

        let sector_count = 5.0;
        let sector_w = world_w / sector_count;
        let sector_h = world_h / sector_count;

        // Adaptive grid spacing: increase spacing at low zoom to reduce line density
        let target_cell = if zoom < 0.1 {
            400.0 // Fewer lines when zoomed way out
        } else if zoom < 0.3 {
            300.0 // Medium spacing at medium-low zoom
        } else {
            200.0 // Default spacing
        };
        
        let subdivisions_x = (sector_w / target_cell).round().max(1.0);
        let subdivisions_y = (sector_h / target_cell).round().max(1.0);
        let grid_size_x = sector_w / subdivisions_x;
        let grid_size_y = sector_h / subdivisions_y;

        let screen_center = Vec2::new(width / 2.0, height / 2.0);

        // Calculate visible grid range, snapped to world origin
        let half_view_w = screen_center.x / zoom;
        let half_view_h = screen_center.y / zoom;
        let start_x = min_x + (((camera_pos.x - half_view_w - min_x) / grid_size_x).floor() * grid_size_x).max(0.0);
        let start_y = min_y + (((camera_pos.y - half_view_h - min_y) / grid_size_y).floor() * grid_size_y).max(0.0);
        let end_x = min_x + ((camera_pos.x + half_view_w - min_x) / grid_size_x).ceil() * grid_size_x;
        let end_y = min_y + ((camera_pos.y + half_view_h - min_y) / grid_size_y).ceil() * grid_size_y;

        let grid_color = if dark_theme { "rgba(255,255,255,0.22)" } else { "rgba(0,0,0,0.18)" };
        ctx.set_stroke_style_str(grid_color);
        ctx.set_line_width(1.0);
        ctx.begin_path();

        // Only snap to pixel boundaries at higher zoom levels where it doesn't cause jitter
        // At very low zoom, accept slightly anti-aliased lines for smooth movement
        let should_snap = zoom >= 0.2;

        // Vertical lines - use integer-based iteration to prevent accumulation errors
        let num_vlines = ((end_x - start_x) / grid_size_x).ceil() as i32;
        for i in 0..=num_vlines {
            let x = start_x + i as f32 * grid_size_x;
            if x > end_x { break; }
            let screen_x = (x - camera_pos.x) * zoom + screen_center.x;
            let final_x = if should_snap { screen_x.round() + 0.5 } else { screen_x };
            ctx.move_to(final_x as f64, 0.0);
            ctx.line_to(final_x as f64, height as f64);
        }

        // Horizontal lines - use integer-based iteration to prevent accumulation errors
        // Horizontal lines - use integer-based iteration to prevent accumulation errors
        let num_hlines = ((end_y - start_y) / grid_size_y).ceil() as i32;
        for i in 0..=num_hlines {
            let y = start_y + i as f32 * grid_size_y;
            if y > end_y { break; }
            let screen_y = (y - camera_pos.y) * zoom + screen_center.y;
            let final_y = if should_snap { screen_y.round() + 0.5 } else { screen_y };
            ctx.move_to(0.0, final_y as f64);
            ctx.line_to(width as f64, final_y as f64);
        }

        ctx.stroke();
    }

    #[inline]
    pub fn draw_background_sectors(
        &self,
        border: (f32, f32, f32, f32),
        camera_pos: Vec2,
        zoom: f32,
        dark_theme: bool,
    ) {
        // Check if we can use cached background sectors
        if let Some((cached_canvas, cached_zoom, cached_x, cached_y, cached_theme)) = self.bg_cache.borrow().as_ref() {
            let zoom_match = (cached_zoom - zoom).abs() < 0.001;
            let pos_match = (cached_x - camera_pos.x).abs() < 1.0 && (cached_y - camera_pos.y).abs() < 1.0;
            let theme_match = *cached_theme == dark_theme;
            
            if zoom_match && pos_match && theme_match {
                // Use cached background - just blit it
                let _ = self.ctx.draw_image_with_html_canvas_element(cached_canvas, 0.0, 0.0);
                return;
            }
        }

        // Need to render - create or reuse offscreen canvas
        let cache_canvas = if let Some((canvas, _, _, _, _)) = self.bg_cache.borrow().as_ref() {
            canvas.clone()
        } else {
            let document = web_sys::window().unwrap().document().unwrap();
            let canvas = document.create_element("canvas").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
            canvas
        };

        cache_canvas.set_width(self.canvas.width());
        cache_canvas.set_height(self.canvas.height());

        let cache_ctx = cache_canvas
            .get_context("2d").unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>().unwrap();

        // Render background sectors to cache canvas
        self.render_background_sectors_to_context(&cache_ctx, self.width(), self.height(), border, camera_pos, zoom, dark_theme);

        // Blit cache to main canvas
        let _ = self.ctx.draw_image_with_html_canvas_element(&cache_canvas, 0.0, 0.0);

        // Update cache
        *self.bg_cache.borrow_mut() = Some((cache_canvas, zoom, camera_pos.x, camera_pos.y, dark_theme));
    }

    #[inline]
    fn render_background_sectors_to_context(
        &self,
        ctx: &CanvasRenderingContext2d,
        width: f32,
        height: f32,
        border: (f32, f32, f32, f32),
        camera_pos: Vec2,
        zoom: f32,
        dark_theme: bool,
    ) {
        let (min_x, min_y, max_x, max_y) = border;
        let world_width = max_x - min_x;
        let world_height = max_y - min_y;
        if world_width <= 0.0 || world_height <= 0.0 {
            return;
        }

        let sector_count = 5;
        let sector_names_x = ["A", "B", "C", "D", "E"];
        let sector_names_y = ["1", "2", "3", "4", "5"];

        let sector_w = world_width / sector_count as f32;
        let sector_h = world_height / sector_count as f32;

        let screen_center = Vec2::new(width / 2.0, height / 2.0);
        let font_size = (sector_w / 3.0 * zoom).max(10.0);

        let world_to_screen = |world: Vec2| -> Vec2 { (world - camera_pos) * zoom + screen_center };
        // Only snap to pixel boundaries at higher zoom levels
        let should_snap = zoom >= 0.2;
        let snap = |v: f32| -> f32 { if should_snap { v.round() + 0.5 } else { v } };

        ctx.set_fill_style_str(if dark_theme { "#666" } else { "#DDD" });
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.set_font(&format!("{}px Ubuntu", font_size.floor()));

        // Sector grid lines
        let line_color = if dark_theme { "rgba(255,255,255,0.18)" } else { "rgba(0,0,0,0.12)" };
        ctx.set_stroke_style_str(line_color);
        ctx.set_line_width(if dark_theme { 3.0 } else { 2.4 });
        ctx.begin_path();
        for i in 1..sector_count {
            let x = min_x + i as f32 * sector_w;
            let y = min_y + i as f32 * sector_h;

            let screen_x = world_to_screen(Vec2::new(x, min_y));
            let screen_x2 = world_to_screen(Vec2::new(x, max_y));
            ctx.move_to(snap(screen_x.x) as f64, snap(screen_x.y) as f64);
            ctx.line_to(snap(screen_x2.x) as f64, snap(screen_x2.y) as f64);

            let screen_y = world_to_screen(Vec2::new(min_x, y));
            let screen_y2 = world_to_screen(Vec2::new(max_x, y));
            ctx.move_to(snap(screen_y.x) as f64, snap(screen_y.y) as f64);
            ctx.line_to(snap(screen_y2.x) as f64, snap(screen_y2.y) as f64);
        }
        ctx.stroke();

        for y in 0..sector_count {
            for x in 0..sector_count {
                let label = format!("{}{}", sector_names_x[x], sector_names_y[y]);
                let world_x = min_x + (x as f32 + 0.5) * sector_w;
                let world_y = min_y + (y as f32 + 0.5) * sector_h;
                let screen_pos = world_to_screen(Vec2::new(world_x, world_y));
                ctx.fill_text(&label, snap(screen_pos.x) as f64, snap(screen_pos.y) as f64).ok();
            }
        }
    }

    #[inline]
    pub fn draw_cell(
        &self,
        cell: &Cell,
        camera_pos: Vec2,
        zoom: f32,
        skin: Option<&HtmlImageElement>,
        show_names: bool,
        show_mass: bool,
        jelly_physics: bool,
        alpha: f32,
    ) {
        let screen_center = Vec2::new(self.width() / 2.0, self.height() / 2.0);
        let screen_pos = (cell.render_position - camera_pos) * zoom + screen_center;
        let radius = cell.render_size * zoom; // size IS the visual radius in world units

        if radius < 1.0 {
            return; // Too small to see
        }

        let (r, g, b) = cell.color;

        // LOD: Skip skins for small cells (< 30px radius)
        let should_render_skin = skin.is_some() && radius >= 30.0;

        if cell.is_virus && !(jelly_physics && !cell.points.is_empty()) {
            self.ctx.set_global_alpha(alpha as f64);
            self.draw_virus(&screen_pos, radius, (r, g, b));
            self.ctx.set_global_alpha(1.0);
        } else {
            self.ctx.set_global_alpha(alpha as f64);
            
            // Circle path — used for fill, clip, and stroke
            self.ctx.begin_path();
            if jelly_physics && !cell.points.is_empty() {
                if let Some(first) = cell.points.first() {
                    let first_screen = (Vec2::new(first.x, first.y) - camera_pos) * zoom + screen_center;
                    self.ctx.move_to(first_screen.x as f64, first_screen.y as f64);
                    for point in &cell.points {
                        let p = (Vec2::new(point.x, point.y) - camera_pos) * zoom + screen_center;
                        self.ctx.line_to(p.x as f64, p.y as f64);
                    }
                    self.ctx.close_path();
                }
            } else {
                self.ctx.arc(
                    screen_pos.x as f64,
                    screen_pos.y as f64,
                    radius as f64,
                    0.0,
                    2.0 * PI as f64,
                ).ok();
            }

            // Fill cell body with its colour
            self.ctx.set_fill_style_str(&format!("rgb({},{},{})", r, g, b));
            self.ctx.fill();

            // Overlay skin image, clipped to the circle (only when loaded and large enough)
            if should_render_skin {
                if let Some(img) = skin {
                    // Cache check: only render if image is complete
                    if img.complete() && img.width() > 0 {
                        self.ctx.save();
                        self.ctx.clip(); // clip region = current path (the circle)
                        // translate + scale so the basic draw_image fills the circle
                        let _ = self.ctx.translate((screen_pos.x - radius) as f64, (screen_pos.y - radius) as f64);
                        let scale = (radius * 2.0) as f64 / img.width() as f64;
                        let _ = self.ctx.scale(scale, scale);
                        self.ctx.draw_image_with_html_image_element(img, 0.0, 0.0).ok();
                        self.ctx.restore(); // remove clip + transform; path still intact for stroke
                    }
                }
            }

            // Border stroke (path persists through save/restore)
            self.ctx.set_stroke_style_str("rgba(0,0,0,0.8)");
            self.ctx.set_line_width(2.0);
            self.ctx.stroke();
            
            // Reset alpha
            self.ctx.set_global_alpha(1.0);
        }

        // LOD: Only draw text for cells above 20px radius (names) or 30px (mass)
        if !cell.is_food {
            if show_names && radius > 20.0 {
                self.draw_text_centered(&cell.name, screen_pos, radius, 16.0);
            }

            if show_mass && radius > 30.0 {
                let mass_text = format!("{:.0}", cell.mass());
                self.draw_text_centered(&mass_text, screen_pos + Vec2::new(0.0, 16.0), radius, 14.0);
            }
        }
    }

    #[inline]
    fn draw_virus(&self, pos: &Vec2, radius: f32, color: (u8, u8, u8)) {
        let sides = 20;
        let spike_factor = 1.15;
        
        self.ctx.begin_path();
        
        for i in 0..sides {
            let angle = (i as f32 / sides as f32) * 2.0 * PI;
            let r = if i % 2 == 0 {
                radius * spike_factor
            } else {
                radius
            };
            
            let x = pos.x + angle.cos() * r;
            let y = pos.y + angle.sin() * r;
            
            if i == 0 {
                self.ctx.move_to(x as f64, y as f64);
            } else {
                self.ctx.line_to(x as f64, y as f64);
            }
        }
        
        self.ctx.close_path();
        
        let (r, g, b) = color;
        self.ctx.set_fill_style_str(&format!("rgb({},{},{})", r, g, b));
        self.ctx.fill();
        
        self.ctx.set_stroke_style_str("rgba(0,0,0,0.8)");
        self.ctx.set_line_width(2.0);
        self.ctx.stroke();
    }

    #[inline]
    fn draw_text_centered(&self, text: &str, pos: Vec2, _max_width: f32, font_size: f32) {
        if text.is_empty() {
            return;
        }

        self.ctx.set_font(&format!("bold {}px Arial", font_size));
        self.ctx.set_text_align("center");
        self.ctx.set_text_baseline("middle");
        
        // Use shadow instead of stroke+fill for 2x performance gain
        self.ctx.set_shadow_blur(4.0);
        self.ctx.set_shadow_color("black");
        self.ctx.set_shadow_offset_x(0.0);
        self.ctx.set_shadow_offset_y(0.0);
        
        self.ctx.set_fill_style_str("white");
        self.ctx.fill_text(text, pos.x as f64, pos.y as f64).ok();
        
        // Reset shadow
        self.ctx.set_shadow_blur(0.0);
    }

    #[inline]
    pub fn draw_border(&self, border: (f32, f32, f32, f32), camera_pos: Vec2, zoom: f32) {
        let (min_x, min_y, max_x, max_y) = border;
        let screen_center = Vec2::new(self.width() / 2.0, self.height() / 2.0);

        let top_left = (Vec2::new(min_x, min_y) - camera_pos) * zoom + screen_center;
        let bottom_right = (Vec2::new(max_x, max_y) - camera_pos) * zoom + screen_center;

        let width = bottom_right.x - top_left.x;
        let height = bottom_right.y - top_left.y;

        self.ctx.set_stroke_style_str("red");
        self.ctx.set_line_width(5.0);
        self.ctx.stroke_rect(
            top_left.x as f64,
            top_left.y as f64,
            width as f64,
            height as f64,
        );
    }
}

// ---------------------------------------------------------------------------
// Minimap — rendered on its own <canvas> element, overlaid bottom-right.
// ---------------------------------------------------------------------------

const MINIMAP_SIZE: u32 = 150;

pub struct Minimap {
    ctx: CanvasRenderingContext2d,
    canvas: HtmlCanvasElement,
    // Static layer cache (background, border, sectors, labels)
    static_cache: RefCell<Option<(HtmlCanvasElement, bool)>>, // (canvas, dark_theme)
}

impl Minimap {
    pub fn new() -> Result<Self, JsValue> {
        let document = web_sys::window()
            .ok_or("No window")?
            .document()
            .ok_or("No document")?;
        let canvas = document
            .get_element_by_id("minimapCanvas")
            .ok_or("minimapCanvas not found")?
            .dyn_into::<HtmlCanvasElement>()?;
        canvas.set_width(MINIMAP_SIZE);
        canvas.set_height(MINIMAP_SIZE);

        let ctx = canvas
            .get_context("2d")?
            .ok_or("Failed to get minimap 2d context")?
            .dyn_into::<CanvasRenderingContext2d>()?;

        Ok(Self {
            ctx,
            canvas,
            static_cache: RefCell::new(None),
        })
    }

    /// Draw the minimap.
    ///
    /// * `border`      – world bounds (min_x, min_y, max_x, max_y)
    /// * `my_cells`    – player's cells as (world_pos, size, rgb)
    /// * `cam_pos`     – current camera centre in world coords
    /// * `cam_zoom`    – current camera zoom factor
    /// * `main_w/h`    – pixel dimensions of the main game canvas
    pub fn draw(
        &self,
        border: (f32, f32, f32, f32),
        my_cells: &[(Vec2, f32, (u8, u8, u8))],
        cam_pos: Vec2,
        cam_zoom: f32,
        main_w: f32,
        main_h: f32,
        dark_theme: bool,
        xray_players: &[(u32, Vec2, f32, (u8, u8, u8), String)],
    ) {
        let size = MINIMAP_SIZE as f64;
        let (min_x, min_y, max_x, max_y) = border;
        let world_w = (max_x - min_x) as f64;
        let world_h = (max_y - min_y) as f64;
        let min_x = min_x as f64;
        let min_y = min_y as f64;

        // Clear canvas
        self.ctx.clear_rect(0.0, 0.0, size, size);

        // Check if we can use cached static layer
        let need_rebuild = if let Some((_, cached_theme)) = self.static_cache.borrow().as_ref() {
            *cached_theme != dark_theme
        } else {
            true
        };

        if need_rebuild {
            self.render_static_layer(size, dark_theme);
        }

        // Blit static layer
        if let Some((static_canvas, _)) = self.static_cache.borrow().as_ref() {
            let _ = self.ctx.draw_image_with_html_canvas_element(static_canvas, 0.0, 0.0);
        }

        // Closure: world pos → minimap pixel pos
        let map = |wx: f64, wy: f64| -> (f64, f64) {
            ((wx - min_x) / world_w * size, (wy - min_y) / world_h * size)
        };

        // --- viewport rectangle (what the main canvas currently shows) ---
        let half_vw = (main_w as f64 / 2.0) / cam_zoom as f64;
        let half_vh = (main_h as f64 / 2.0) / cam_zoom as f64;
        let (vx, vy) = map(cam_pos.x as f64 - half_vw, cam_pos.y as f64 - half_vh);
        let (vx2, vy2) = map(cam_pos.x as f64 + half_vw, cam_pos.y as f64 + half_vh);

        self.ctx.set_stroke_style_str("rgba(255,255,255,0.8)");
        self.ctx.set_line_width(2.0);
        self.ctx.stroke_rect(vx, vy, vx2 - vx, vy2 - vy);

        // --- highlight current sector ---
        let (mx, my) = map(cam_pos.x as f64, cam_pos.y as f64);
        let sector_w = size / 5.0;
        let sector_h = size / 5.0;
        let sector_x = (mx / sector_w).floor().clamp(0.0, 4.0);
        let sector_y = (my / sector_h).floor().clamp(0.0, 4.0);
        self.ctx.set_fill_style_str("yellow");
        self.ctx.set_global_alpha(0.3);
        self.ctx.fill_rect(sector_x * sector_w, sector_y * sector_h, sector_w, sector_h);
        self.ctx.set_global_alpha(1.0);

        // --- player cells ---
        let x_scale = size / world_w;
        let y_scale = size / world_h;
        for &(pos, cell_size, (r, g, b)) in my_cells {
            let (mx, my) = map(pos.x as f64, pos.y as f64);
            // Scale dot radius proportionally with cell size, clamped to minimum for visibility
            let dot_r = (cell_size as f64 * (x_scale + y_scale) / 2.0).max(1.5);

            self.ctx.begin_path();
            let _ = self.ctx.arc(mx, my, dot_r, 0.0, TAU);
            self.ctx.set_fill_style_str(&format!("rgb({},{},{})", r, g, b));
            self.ctx.fill();

            // thin dark outline so dots are visible on any background
            self.ctx.set_stroke_style_str("rgba(0,0,0,0.6)");
            self.ctx.set_line_width(1.0);
            self.ctx.stroke();
        }

        // --- xray players ---
        if !xray_players.is_empty() {
            self.ctx.save(); // Isolate xray rendering state
            let mut drawn_names: HashSet<String> = HashSet::new();
            let pulse = (utils::now() * 0.005).sin();
            let x_scale = size / world_w;
            let y_scale = size / world_h;
            for (_, pos, cell_size, (r, g, b), name) in xray_players {
                self.ctx.save(); // Isolate each player's rendering state
                let (mx, my) = map(pos.x as f64, pos.y as f64);
                let dot_r = (cell_size.max(20.0) as f64 * (x_scale + y_scale) / 2.0).max(1.5);
                let alpha = (0.7 + 0.3 * pulse) as f64;

                self.ctx.set_fill_style_str(&format!("rgb({},{},{})", r, g, b));
                self.ctx.set_global_alpha(alpha);
                self.ctx.begin_path();
                let _ = self.ctx.arc(mx, my, dot_r, 0.0, TAU);
                self.ctx.fill();

                self.ctx.set_global_alpha(1.0);
                self.ctx.set_stroke_style_str("#FFF");
                self.ctx.set_line_width(1.0);
                self.ctx.begin_path();
                let _ = self.ctx.arc(mx, my, dot_r + 1.0, 0.0, TAU);
                self.ctx.stroke();

                if !name.is_empty() && drawn_names.insert(name.clone()) {
                    self.ctx.set_fill_style_str(if dark_theme { "#FFF" } else { "#000" });
                    self.ctx.set_global_alpha(0.9);
                    self.ctx.set_font(&format!("{}px Ubuntu", dot_r.max(8.0)));
                    self.ctx.set_text_align("center");
                    self.ctx.set_text_baseline("middle");
                    self.ctx.fill_text(name, mx, my - dot_r - 10.0).ok();
                }
                
                self.ctx.restore(); // Restore state after each player
            }
            self.ctx.restore(); // Restore state after xray section
        }
    }

    fn render_static_layer(&self, size: f64, dark_theme: bool) {
        // Create or reuse offscreen canvas for static elements
        let static_canvas = if let Some((canvas, _)) = self.static_cache.borrow().as_ref() {
            canvas.clone()
        } else {
            let document = web_sys::window().unwrap().document().unwrap();
            let canvas = document.create_element("canvas").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
            canvas
        };

        static_canvas.set_width(MINIMAP_SIZE);
        static_canvas.set_height(MINIMAP_SIZE);

        let static_ctx = static_canvas
            .get_context("2d").unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>().unwrap();

        // --- background ---
        static_ctx.set_fill_style_str(if dark_theme { "rgba(0,0,0,0.7)" } else { "rgba(255,255,255,0.7)" });
        static_ctx.fill_rect(0.0, 0.0, size, size);

        // --- world-border outline ---
        static_ctx.set_stroke_style_str(if dark_theme { "rgba(255,255,255,0.35)" } else { "rgba(0,0,0,0.35)" });
        static_ctx.set_line_width(1.0);
        static_ctx.stroke_rect(0.0, 0.0, size, size);

        // --- sector labels ---
        let sector_names_x = ["A", "B", "C", "D", "E"];
        let sector_names_y = ["1", "2", "3", "4", "5"];
        let sector_w = size / 5.0;
        let sector_h = size / 5.0;
        let sector_font = (sector_w.min(sector_h) / 3.0).max(8.0);

        // Sector grid lines
        let grid_color = if dark_theme { "rgba(255,255,255,0.22)" } else { "rgba(0,0,0,0.14)" };
        static_ctx.set_stroke_style_str(grid_color);
        static_ctx.set_line_width(if dark_theme { 1.5 } else { 1.2 });
        static_ctx.begin_path();
        for i in 1..5 {
            let x = i as f64 * sector_w;
            let y = i as f64 * sector_h;
            static_ctx.move_to(x, 0.0);
            static_ctx.line_to(x, size);
            static_ctx.move_to(0.0, y);
            static_ctx.line_to(size, y);
        }
        static_ctx.stroke();

        static_ctx.set_fill_style_str(if dark_theme { "#666" } else { "#DDD" });
        static_ctx.set_text_align("center");
        static_ctx.set_text_baseline("middle");
        static_ctx.set_font(&format!("{}px Ubuntu", sector_font.floor()));

        for x in 0..5 {
            for y in 0..5 {
                let label = format!("{}{}", sector_names_x[x], sector_names_y[y]);
                let lx = (x as f64 + 0.5) * sector_w;
                let ly = (y as f64 + 0.5) * sector_h;
                static_ctx.fill_text(&label, lx, ly).ok();
            }
        }

        // Update cache
        *self.static_cache.borrow_mut() = Some((static_canvas, dark_theme));
    }
}
