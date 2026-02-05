// DOM manipulation, overlays, menus, chat
use web_sys::{Document, Element, HtmlInputElement};
use wasm_bindgen::{JsCast, JsValue};

pub struct UI {
    document: Document,
}

impl UI {
    pub fn new(document: Document) -> Self {
        Self { document }
    }

    fn get_el(&self, id: &str) -> Option<Element> {
        self.document.get_element_by_id(id)
    }

    /// Update the leaderboard DOM list.
    pub fn update_leaderboard(&self, entries: &[(bool, String)]) {
        let list = match self.get_el("leaderboardList") {
            Some(el) => el,
            None => return,
        };
        let mut html = String::new();
        for (is_me, name) in entries {
            let escaped = html_escape(name);
            if *is_me {
                html.push_str(&format!("<li class=\"my-1 font-bold text-green-400\">{}</li>", escaped));
            } else {
                html.push_str(&format!("<li class=\"my-1\">{}</li>", escaped));
            }
        }
        list.set_inner_html(&html);
    }

    /// Append a single chat message to the chat box and auto-scroll.
    pub fn show_chat_message(&self, name: &str, message: &str, color: (u8, u8, u8)) {
        let chat_box = match self.get_el("chatBox") {
            Some(el) => el,
            None => return,
        };
        let div = match self.document.create_element("div") {
            Ok(el) => el,
            Err(_) => return,
        };
        let (r, g, b) = color;
        div.set_class_name("my-1");
        div.set_inner_html(&format!(
            "<span class=\"theme-text\"><span style=\"color:rgb({},{},{})\"><b>{}</b></span>: {}</span>",
            r, g, b,
            html_escape(name),
            html_escape(message),
        ));
        chat_box.append_child(&div).ok();
        // Auto-scroll to bottom
        chat_box.set_scroll_top(chat_box.scroll_height());
    }

    /// Update the HUD stats (FPS / Score / Cells).
    pub fn update_stats(&self, fps: u32, score: f32, cells: usize) {
        if let Some(el) = self.get_el("fps") {
            el.set_inner_html(&fps.to_string());
        }
        if let Some(el) = self.get_el("score") {
            el.set_inner_html(&format!("{:.0}", score));
        }
        if let Some(el) = self.get_el("cellCount") {
            el.set_inner_html(&cells.to_string());
        }
    }

    /// Show the login overlay (on death or initial load), pre-filling the nick + skin inputs.
    pub fn show_login_overlay(&self, nick: &str, skin: Option<&str>) {
        // Unhide overlay (remove only "hidden"; preserve all layout classes)
        if let Some(overlay) = self.get_el("loginOverlay") {
            overlay.class_list().remove(&js_sys::Array::of1(&JsValue::from("hidden"))).ok();
        }
        // Pre-fill nick
        if let Some(input) = self.get_el("nickInput") {
            if let Ok(input) = input.dyn_into::<HtmlInputElement>() {
                input.set_value(nick);
            }
        }
        // Pre-fill skin
        if let Some(input) = self.get_el("skinInput") {
            if let Ok(input) = input.dyn_into::<HtmlInputElement>() {
                input.set_value(skin.unwrap_or(""));
            }
        }
        // Hide game HUD (add "hidden"; preserve all layout classes)
        for id in &["stats", "leaderboard", "instructions", "chatBox", "chatInputRow", "minimapCanvas"] {
            if let Some(el) = self.get_el(id) {
                el.class_list().add(&js_sys::Array::of1(&JsValue::from("hidden"))).ok();
            }
        }
    }

    /// Focus the chat input field
    pub fn focus_chat_input(&self) {
        if let Some(input) = self.get_el("chatInput") {
            if let Ok(input) = input.dyn_into::<HtmlInputElement>() {
                let _ = input.focus();
            }
        }
    }

    /// Update the server stats display
    pub fn update_server_stats(&self, stats: &crate::game::ServerStats, latency: Option<f64>) {
        // Show the server stats section
        if let Some(section) = self.get_el("serverStatsSection") {
            // Remove the inline style that hides it
            if let Ok(el) = section.dyn_into::<web_sys::HtmlElement>() {
                let _ = el.set_attribute("style", "display: block;");
            }
        }

        // Update server name and mode
        if let Some(el) = self.get_el("serverName") {
            el.set_inner_html(&format!("{} ({})", 
                html_escape(&stats.name), 
                html_escape(&stats.mode)));
        }

        // Update player counts
        if let Some(el) = self.get_el("serverPlayers") {
            el.set_inner_html(&format!("{} / {} players", 
                stats.players_total, 
                stats.players_limit));
        }

        if let Some(el) = self.get_el("serverAlive") {
            el.set_inner_html(&format!("{} playing", stats.players_alive));
        }

        if let Some(el) = self.get_el("serverSpectating") {
            el.set_inner_html(&format!("{} spectating", stats.players_spect));
        }

        // Calculate and display server load
        if let Some(el) = self.get_el("serverLoad") {
            if let Ok(update_val) = stats.update.parse::<f64>() {
                let load = update_val * 2.5;
                // Format uptime
                let uptime_str = format_uptime(stats.uptime);
                el.set_inner_html(&format!("{:.1}% load @ {}", load, uptime_str));
            }
        }

        // Display latency if available
        if let Some(el) = self.get_el("serverLatency") {
            if let Some(lat) = latency {
                el.set_inner_html(&format!("Latency: {:.0}ms", lat));
            }
        }
    }
}

/// Format uptime seconds into a human-readable string
fn format_uptime(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Escape HTML special characters to prevent XSS from server-supplied strings.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
