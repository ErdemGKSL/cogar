// Helper utilities, LERP, math functions, logging

/// Linear interpolation between two values
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Get the current high-precision timestamp in milliseconds
pub fn now() -> f64 {
    web_sys::window()
        .expect("no global window")
        .performance()
        .expect("no performance")
        .now()
}

/// Log to browser console
#[macro_export]
macro_rules! console_log {
    ($($t:tt)*) => {
        web_sys::console::log_1(&format!($($t)*).into());
    }
}

/// Clamp a value between min and max
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}
