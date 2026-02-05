// WASM client entry point for native-agar
// This crate provides a modular, systematic Rust implementation of the Cigar2 client

use wasm_bindgen::prelude::*;
use std::rc::Rc;
use std::rc::Weak;
use std::cell::RefCell;
use web_sys::{window, KeyboardEvent, MouseEvent, MessageEvent, HtmlCanvasElement, HtmlInputElement, HtmlButtonElement, Element, WheelEvent, WebSocket, CloseEvent};
use js_sys::{ArrayBuffer, Uint8Array};
use glam::Vec2;

// Module structure - each module handles a specific concern
mod network;  // WebSocket connection, packet handling
mod game;     // Game state, cell management, world representation
mod render;   // Canvas rendering, drawing cells/grid/UI
mod camera;   // Viewport, zoom, smooth follow
mod input;    // Mouse and keyboard event handling
mod ui;       // DOM manipulation, overlays, menus
mod utils;    // Helper functions, LERP, math utilities

// Re-export the main entry point
pub use game::GameClient;

/// Initialize panic hook for better error messages in the browser console
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Create and return a GameClient that JS can interact with
#[wasm_bindgen]
pub struct GameClientWrapper {
    client: Rc<RefCell<GameClient>>,
}

#[wasm_bindgen]
impl GameClientWrapper {
    /// Create a new game client
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str, server_url: &str) -> Result<GameClientWrapper, JsValue> {
        init();

        let client = GameClient::new(canvas_id, server_url)?;
        let client_rc = Rc::new(RefCell::new(client));

        // Setup WebSocket message handler
        setup_websocket_handler(client_rc.clone())?;

        // Setup animation loop
        setup_animation_loop(client_rc.clone())?;

        // Setup input handlers
        setup_input_handlers(client_rc.clone())?;

        // Setup chat handlers
        setup_chat_handlers(client_rc.clone())?;

        // Setup zoom handlers
        setup_zoom_handlers(client_rc.clone())?;

        // Setup settings handlers
        setup_settings_handlers(client_rc.clone())?;

        // Setup canvas resize handler
        setup_resize_handler(canvas_id)?;

        Ok(GameClientWrapper {
            client: client_rc,
        })
    }

    /// Spawn with a nickname
    pub fn spawn(&self, nick: &str) {
        // Queue the spawn request - game loop will process it
        *self.client.borrow().pending_spawn().borrow_mut() = Some(nick.to_string());
    }

    /// Check if player is alive
    pub fn is_alive(&self) -> bool {
        self.client.borrow().is_alive()
    }

    /// Get the number of cells the player currently has
    pub fn cell_count(&self) -> usize {
        self.client.borrow().my_cells_count()
    }

    /// Send a chat message to the server
    pub fn send_chat(&self, message: &str) {
        self.client.borrow().send_chat_message(message);
    }

    /// Get the underlying WebSocket for connection status checks
    pub fn websocket(&self) -> web_sys::WebSocket {
        self.client.borrow().websocket()
    }
}

struct ReconnectState {
    delay_ms: i32,
    max_delay_ms: i32,
    scheduled: bool,
}

fn attach_websocket_handlers(
    client: Rc<RefCell<GameClient>>,
    ws: WebSocket,
    reconnect_state: Rc<RefCell<ReconnectState>>,
) -> Result<(), JsValue> {
    // Get shared resources that don't require borrowing client
    let packet_queue = client.borrow().packet_queue();
    let ws_open_flag = client.borrow().ws_open_flag();

    let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
        if let Ok(buffer) = event.data().dyn_into::<ArrayBuffer>() {
            let array = Uint8Array::new(&buffer);
            let mut data = vec![0u8; array.length() as usize];
            array.copy_to(&mut data);

            // Push packet to queue - game loop will process it
            packet_queue.borrow_mut().push(data);
        }
    }) as Box<dyn FnMut(MessageEvent)>);
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // onopen - set flag and reset reconnect delay
    let onopen_state = reconnect_state.clone();
    let onopen = Closure::wrap(Box::new(move |_event: JsValue| {
        web_sys::console::log_1(&"WebSocket connected".into());
        // Set flag for game loop to process
        ws_open_flag.set(true);
        // Reset reconnect state on successful connection
        if let Ok(mut state) = onopen_state.try_borrow_mut() {
            state.delay_ms = 1000;
            state.scheduled = false;
        }
    }) as Box<dyn FnMut(JsValue)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // onerror
    let onerror = Closure::wrap(Box::new(move |e: JsValue| {
        web_sys::console::error_1(&format!("WebSocket error: {:?}", e).into());
    }) as Box<dyn FnMut(JsValue)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // onclose - schedule reconnect
    let client_weak: Weak<RefCell<GameClient>> = Rc::downgrade(&client);
    let ws_close_flag = client.borrow().ws_close_flag();
    let onclose_state = reconnect_state.clone();
    let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
        web_sys::console::log_1(&format!("WebSocket closed: {}", event.code()).into());

        // Set flag for game loop to process disconnect
        ws_close_flag.set(true);

        let delay = {
            let mut state = onclose_state.borrow_mut();
            if state.scheduled {
                return;
            }
            state.scheduled = true;
            let current = state.delay_ms;
            state.delay_ms = ((state.delay_ms as f64) * 1.5).min(state.max_delay_ms as f64) as i32;
            current
        };

        if let Some(window) = web_sys::window() {
            let attempt_client = client_weak.clone();
            let attempt_state = onclose_state.clone();
            let callback = Closure::wrap(Box::new(move || {
                if let Some(client_rc) = attempt_client.upgrade() {
                    // Use try_borrow_mut to avoid panic if client is borrowed elsewhere
                    match client_rc.try_borrow_mut() {
                        Ok(mut client) => {
                            match client.reconnect() {
                                Ok(new_ws) => {
                                    drop(client); // Release borrow before attaching handlers
                                    // Create a fresh reconnect state for the new connection
                                    let new_reconnect_state = Rc::new(RefCell::new(ReconnectState {
                                        delay_ms: attempt_state.borrow().delay_ms,
                                        max_delay_ms: attempt_state.borrow().max_delay_ms,
                                        scheduled: false,
                                    }));
                                    if let Err(e) = attach_websocket_handlers(client_rc.clone(), new_ws, new_reconnect_state) {
                                        web_sys::console::error_1(&format!("Failed to attach handlers: {:?}", e).into());
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::error_1(&format!("Reconnect failed: {:?}", e).into());
                                    // Reset scheduled flag so we can try again
                                    if let Ok(mut state) = attempt_state.try_borrow_mut() {
                                        state.scheduled = false;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            web_sys::console::log_1(&"Reconnect deferred: client busy".into());
                            // Client is busy, don't panic - we'll try next time
                            if let Ok(mut state) = attempt_state.try_borrow_mut() {
                                state.scheduled = false;
                            }
                        }
                    }
                }
            }) as Box<dyn FnMut()>);
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                delay,
            );
            callback.forget();
        }
    }) as Box<dyn FnMut(CloseEvent)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    Ok(())
}

fn setup_websocket_handler(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let ws = client.borrow().websocket().clone();
    let reconnect_state = Rc::new(RefCell::new(ReconnectState {
        delay_ms: 1000,
        max_delay_ms: 5000,
        scheduled: false,
    }));
    attach_websocket_handlers(client, ws, reconnect_state)
}

fn setup_animation_loop(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let window = window().ok_or("No window")?;

    // Create animation frame closure
    let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    let client_clone = client.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        // Update and render - now safe to use borrow_mut since WebSocket only queues
        if let Err(e) = client_clone.borrow_mut().update() {
            web_sys::console::error_1(&format!("Update error: {:?}", e).into());
        }

        // Request next frame
        if let Some(win) = web_sys::window() {
            win
                .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref())
                .ok();
        }
    }) as Box<dyn FnMut()>));

    // Start the loop
    window
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;

    Ok(())
}

/// Returns true when a text input element has focus (nick input or chat input).
/// Used to suppress game key bindings while the user is typing.
fn is_text_input_focused() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|el| el.tag_name().eq_ignore_ascii_case("INPUT"))
        .unwrap_or(false)
}

fn setup_input_handlers(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let window = window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;

    // Get the shared input state
    let input_state = client.borrow().input_state();

    // Mouse move handler
    {
        let input_clone = input_state.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let x = event.client_x() as f32;
            let y = event.client_y() as f32;
            input_clone.borrow_mut().mouse_pos = Vec2::new(x, y);
        }) as Box<dyn FnMut(_)>);

        document.add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Keydown handler
    {
        let input_clone = input_state.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if is_text_input_focused() {
                return; // Don't send game commands while typing
            }
            let key = event.key();
            let mut input = input_clone.borrow_mut();
            match key.as_str() {
                " " => { event.prevent_default(); input.space_pressed = true; }
                "w" | "W" => input.w_pressed = true,
                "q" | "Q" => input.q_pressed = true,
                "e" | "E" => input.e_pressed = true,
                "r" | "R" => input.r_pressed = true,
                "t" | "T" => input.t_pressed = true,
                "p" | "P" => input.p_pressed = true,
                "Enter" => input.enter_pressed = true,
                "Escape" => input.escape_pressed = true,
                _ => {}
            }
        }) as Box<dyn FnMut(_)>);

        document.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Keyup handler
    {
        let input_clone = input_state.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if is_text_input_focused() {
                return;
            }
            let key = event.key();
            let mut input = input_clone.borrow_mut();
            match key.as_str() {
                " " => input.space_pressed = false,
                "w" | "W" => input.w_pressed = false,
                "q" | "Q" => input.q_pressed = false,
                "e" | "E" => input.e_pressed = false,
                "r" | "R" => input.r_pressed = false,
                "t" | "T" => input.t_pressed = false,
                "p" | "P" => input.p_pressed = false,
                "Enter" => input.enter_pressed = false,
                "Escape" => input.escape_pressed = false,
                _ => {}
            }
        }) as Box<dyn FnMut(_)>);

        document.add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

fn setup_chat_handlers(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let window = window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;

    let chat_input = document
        .get_element_by_id("chatInput")
        .ok_or("chatInput not found")?
        .dyn_into::<HtmlInputElement>()?;
    let chat_row = document
        .get_element_by_id("chatInputRow")
        .ok_or("chatInputRow not found")?
        .dyn_into::<Element>()?;
    let chat_send = document
        .get_element_by_id("chatSend")
        .ok_or("chatSend not found")?
        .dyn_into::<HtmlButtonElement>()?;
    let login_overlay = document
        .get_element_by_id("loginOverlay")
        .ok_or("loginOverlay not found")?
        .dyn_into::<Element>()?;

    let hidden = JsValue::from("hidden");
    let hidden_arr = js_sys::Array::of1(&hidden);

    // Ensure chat input is visible by default (when overlay hidden)
    if login_overlay.class_list().contains("hidden") {
        chat_row.class_list().remove(&hidden_arr).ok();
    }

    // Enter sends, Escape dismisses
    {
        let chat_input_outer = chat_input.clone();
        let chat_input_inner = chat_input.clone();
        let _chat_row = chat_row.clone();
        let client = client.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            let key = event.key();
            if key == "Enter" {
                event.prevent_default();
                let msg = chat_input_inner.value().trim().to_string();
                if !msg.is_empty() {
                    client.borrow().send_chat_message(&msg);
                }
                chat_input_inner.set_value("");
            } else if key == "Escape" {
                event.prevent_default();
                chat_input_inner.set_value("");
                let _ = chat_input_inner.blur();
            }
        }) as Box<dyn FnMut(_)>);

        chat_input_outer.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Send button click
    {
        let chat_input = chat_input.clone();
        let _chat_row = chat_row.clone();
        let client = client.clone();
        let closure = Closure::wrap(Box::new(move |_| {
            let msg = chat_input.value().trim().to_string();
            if !msg.is_empty() {
                client.borrow().send_chat_message(&msg);
            }
            chat_input.set_value("");
            let _ = chat_input.blur();
        }) as Box<dyn FnMut(JsValue)>);

        chat_send.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

fn setup_zoom_handlers(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let window = window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;

    let closure = Closure::wrap(Box::new(move |event: WheelEvent| {
        if is_text_input_focused() {
            return;
        }
        event.prevent_default();
        let delta = event.delta_y();
        // Negative delta_y = zoom in, positive = zoom out
        let factor = if delta < 0.0 { 1.1 } else { 0.9 };
        client.borrow_mut().adjust_zoom(factor);
    }) as Box<dyn FnMut(_)>);

    document.add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

fn setup_settings_handlers(client: Rc<RefCell<GameClient>>) -> Result<(), JsValue> {
    let window = window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;

    let show_skins = document
        .get_element_by_id("settingShowSkins")
        .ok_or("settingShowSkins not found")?
        .dyn_into::<HtmlInputElement>()?;
    let show_names = document
        .get_element_by_id("settingShowNames")
        .ok_or("settingShowNames not found")?
        .dyn_into::<HtmlInputElement>()?;
    let show_mass = document
        .get_element_by_id("settingShowMass")
        .ok_or("settingShowMass not found")?
        .dyn_into::<HtmlInputElement>()?;
    let show_grid = document
        .get_element_by_id("settingShowGrid")
        .ok_or("settingShowGrid not found")?
        .dyn_into::<HtmlInputElement>()?;
    let show_background_sectors = document
        .get_element_by_id("settingShowBackgroundSectors")
        .ok_or("settingShowBackgroundSectors not found")?
        .dyn_into::<HtmlInputElement>()?;
    let show_minimap = document
        .get_element_by_id("settingShowMinimap")
        .ok_or("settingShowMinimap not found")?
        .dyn_into::<HtmlInputElement>()?;
    let dark_theme = document
        .get_element_by_id("settingDarkTheme")
        .ok_or("settingDarkTheme not found")?
        .dyn_into::<HtmlInputElement>()?;

    let minimap_canvas = document
        .get_element_by_id("minimapCanvas")
        .ok_or("minimapCanvas not found")?
        .dyn_into::<Element>()?;

    let hidden_bang = js_sys::Array::of1(&JsValue::from("hidden!"));

    // Apply initial settings
    {
        let mut client = client.borrow_mut();
        client.set_show_skins(show_skins.checked());
        client.set_show_names(show_names.checked());
        client.set_show_mass(show_mass.checked());
        client.set_show_grid(show_grid.checked());
        client.set_show_background_sectors(show_background_sectors.checked());
        client.set_show_minimap(show_minimap.checked());
        client.set_dark_theme(dark_theme.checked());
    }
    if show_minimap.checked() {
        minimap_canvas.class_list().remove(&hidden_bang).ok();
    } else {
        minimap_canvas.class_list().add(&hidden_bang).ok();
    }

    let bind_checkbox = |input: HtmlInputElement, mut f: Box<dyn FnMut(bool)>| {
        let input_clone = input.clone();
        let closure = Closure::wrap(Box::new(move |_| {
            f(input_clone.checked());
        }) as Box<dyn FnMut(JsValue)>);
        input.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref()).ok();
        closure.forget();
    };

    // Show skins
    {
        let client = client.clone();
        bind_checkbox(show_skins.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_skins(v);
        }));
    }
    // Show names
    {
        let client = client.clone();
        bind_checkbox(show_names.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_names(v);
        }));
    }
    // Show mass
    {
        let client = client.clone();
        bind_checkbox(show_mass.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_mass(v);
        }));
    }
    // Show grid
    {
        let client = client.clone();
        bind_checkbox(show_grid.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_grid(v);
        }));
    }
    // Show background sectors
    {
        let client = client.clone();
        bind_checkbox(show_background_sectors.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_background_sectors(v);
        }));
    }
    // Show minimap
    {
        let client = client.clone();
        let minimap_canvas = minimap_canvas.clone();
        let hidden_bang = hidden_bang.clone();
        bind_checkbox(show_minimap.clone(), Box::new(move |v| {
            client.borrow_mut().set_show_minimap(v);
            if v {
                minimap_canvas.class_list().remove(&hidden_bang).ok();
            } else {
                minimap_canvas.class_list().add(&hidden_bang).ok();
            }
        }));
    }
    // Dark theme
    {
        let client = client.clone();
        bind_checkbox(dark_theme.clone(), Box::new(move |v| {
            client.borrow_mut().set_dark_theme(v);
        }));
    }

    Ok(())
}

/// Resize the canvas when the browser window is resized.
fn setup_resize_handler(canvas_id: &str) -> Result<(), JsValue> {
    let win = window().ok_or("No window")?;
    let id = canvas_id.to_string();

    let closure = Closure::wrap(Box::new(move || {
        if let (Some(win), Some(doc)) = (web_sys::window(), web_sys::window().and_then(|w| w.document())) {
            if let Some(canvas_el) = doc.get_element_by_id(&id) {
                if let Ok(canvas) = canvas_el.dyn_into::<HtmlCanvasElement>() {
                    if let Ok(w) = win.inner_width() {
                        canvas.set_width(w.as_f64().unwrap_or(800.0) as u32);
                    }
                    if let Ok(h) = win.inner_height() {
                        canvas.set_height(h.as_f64().unwrap_or(600.0) as u32);
                    }
                }
            }
        }
    }) as Box<dyn FnMut()>);

    win.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}
