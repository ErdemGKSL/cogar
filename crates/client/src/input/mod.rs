// Mouse and keyboard input handling
use glam::Vec2;

pub struct Input {
    pub mouse_pos: Vec2,
    pub space_pressed: bool,
    pub w_pressed: bool,
    pub q_pressed: bool,
    pub e_pressed: bool,
    pub r_pressed: bool,
    pub t_pressed: bool,
    pub p_pressed: bool,
    pub enter_pressed: bool,
    pub escape_pressed: bool,
    // Previous frame states for edge detection
    pub prev_space_pressed: bool,
    pub prev_w_pressed: bool,
    pub prev_q_pressed: bool,
    pub prev_e_pressed: bool,
    pub prev_r_pressed: bool,
    pub prev_t_pressed: bool,
    pub prev_p_pressed: bool,
    pub prev_enter_pressed: bool,
    pub prev_escape_pressed: bool,
}

impl Input {
    pub fn new() -> Self {
        Self {
            mouse_pos: Vec2::ZERO,
            space_pressed: false,
            w_pressed: false,
            q_pressed: false,
            e_pressed: false,
            r_pressed: false,
            t_pressed: false,
            p_pressed: false,
            enter_pressed: false,
            escape_pressed: false,
            prev_space_pressed: false,
            prev_w_pressed: false,
            prev_q_pressed: false,
            prev_e_pressed: false,
            prev_r_pressed: false,
            prev_t_pressed: false,
            prev_p_pressed: false,
            prev_enter_pressed: false,
            prev_escape_pressed: false,
        }
    }
    
    /// Update previous frame state - call this once per frame
    pub fn update_previous_state(&mut self) {
        self.prev_space_pressed = self.space_pressed;
        self.prev_w_pressed = self.w_pressed;
        self.prev_q_pressed = self.q_pressed;
        self.prev_e_pressed = self.e_pressed;
        self.prev_r_pressed = self.r_pressed;
        self.prev_t_pressed = self.t_pressed;
        self.prev_p_pressed = self.p_pressed;
        self.prev_enter_pressed = self.enter_pressed;
        self.prev_escape_pressed = self.escape_pressed;
    }
    
    /// Check if key was just pressed (transition from not pressed to pressed)
    pub fn space_just_pressed(&self) -> bool {
        self.space_pressed && !self.prev_space_pressed
    }
    
    pub fn w_just_pressed(&self) -> bool {
        self.w_pressed && !self.prev_w_pressed
    }
    
    pub fn q_just_pressed(&self) -> bool {
        self.q_pressed && !self.prev_q_pressed
    }
    
    pub fn e_just_pressed(&self) -> bool {
        self.e_pressed && !self.prev_e_pressed
    }
    
    pub fn r_just_pressed(&self) -> bool {
        self.r_pressed && !self.prev_r_pressed
    }
    
    pub fn t_just_pressed(&self) -> bool {
        self.t_pressed && !self.prev_t_pressed
    }
    
    pub fn p_just_pressed(&self) -> bool {
        self.p_pressed && !self.prev_p_pressed
    }
    
    pub fn enter_just_pressed(&self) -> bool {
        self.enter_pressed && !self.prev_enter_pressed
    }
    
    pub fn escape_just_pressed(&self) -> bool {
        self.escape_pressed && !self.prev_escape_pressed
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}
