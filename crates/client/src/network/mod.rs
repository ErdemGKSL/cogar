// WebSocket connection and binary protocol handling
use wasm_bindgen::prelude::*;
use web_sys::{WebSocket, BinaryType};
use protocol::BinaryWriter;
use js_sys::Uint8Array;

pub struct Connection {
    ws: WebSocket,
    url: String,
    scramble_x: i32,
    scramble_y: i32,
    scramble_id: u32,
    protocol_version: u8,
}

impl Connection {
    pub fn new(url: &str) -> Result<Self, JsValue> {
        // Construct WebSocket URL with proper protocol
        let ws_url = if url.starts_with("ws://") || url.starts_with("wss://") {
            url.to_string()
        } else {
            // Check if we're on HTTPS
            let is_https = web_sys::window()
                .and_then(|w| w.location().protocol().ok())
                .map(|p| p == "https:")
                .unwrap_or(false);
            
            format!("ws{}://{}", if is_https { "s" } else { "" }, url)
        };
        
        web_sys::console::log_1(&format!("Connecting to: {}", ws_url).into());
        let ws = WebSocket::new(&ws_url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        
        Ok(Self {
            ws,
            url: ws_url,
            scramble_x: 0,
            scramble_y: 0,
            scramble_id: 0,
            protocol_version: 6, // Match JS client (Cigar2)
        })
    }

    pub fn websocket(&self) -> &WebSocket {
        &self.ws
    }

    pub fn reconnect(&mut self) -> Result<WebSocket, JsValue> {
        // Clean up old websocket
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onerror(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();

        web_sys::console::log_1(&format!("Reconnecting to: {}", self.url).into());
        let ws = WebSocket::new(&self.url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        self.ws = ws;
        Ok(self.ws.clone())
    }

    pub fn set_scramble(&mut self, x: i32, y: i32, id: u32) {
        self.scramble_x = x;
        self.scramble_y = y;
        self.scramble_id = id;
    }

    pub fn scramble_x(&self) -> i32 {
        self.scramble_x
    }

    pub fn scramble_y(&self) -> i32 {
        self.scramble_y
    }

    pub fn scramble_id(&self) -> u32 {
        self.scramble_id
    }

    fn send_bytes(&self, data: &[u8]) -> Result<(), JsValue> {
        // Check if WebSocket is ready (OPEN state = 1)
        if self.ws.ready_state() != 1 {
            return Err(JsValue::from_str("WebSocket not ready"));
        }
        let array = Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);
        self.ws.send_with_array_buffer(&array.buffer())
    }

    /// Send handshake (0xFF + key 1 for protocol <= 6)
    pub fn send_handshake(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0xFF); // HandshakeKey opcode
        writer.put_u32(1);   // Key = 1 for protocol <= 6
        self.send_bytes(writer.as_slice())
    }

    /// Send protocol version (0xFE + version as u32 — server expects exactly 5 bytes)
    pub fn send_protocol_version(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0xFE);
        writer.put_u32(self.protocol_version as u32);
        self.send_bytes(writer.as_slice())
    }

    /// Send spawn request (0x00 + nick as UTF-8, protocol <= 6)
    pub fn send_spawn(&self, nick: &str) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x00); // Join opcode
        writer.put_string_utf8(nick);
        self.send_bytes(writer.as_slice())
    }

    /// Send mouse position (0x10 + x + y)
    /// Coordinates are already in scrambled world space — server subtracts scramble on receipt.
    pub fn send_mouse(&self, x: f32, y: f32) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x10);
        writer.put_i32(x as i32);
        writer.put_i32(y as i32);

        // Protocol >=6: 13-byte mouse packet (4 extra zero bytes)
        if self.protocol_version >= 6 {
            writer.put_u32(0);
        }

        self.send_bytes(writer.as_slice())
    }

    /// Send split request (Space key, 0x11)
    pub fn send_split(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x11); // Split opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send eject request (W key, 0x15)
    pub fn send_eject(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x15); // Eject opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send Q key (0x12)
    pub fn send_q(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x12); // KeyQ opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send E key (0x16)
    pub fn send_e(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x16); // KeyE opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send R key (0x17)
    pub fn send_r(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x17); // KeyR opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send T key (0x18)
    pub fn send_t(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x18); // KeyT opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send P key (0x19)
    pub fn send_p(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x19); // KeyP opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send spectate request (0x01)
    pub fn send_spectate(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x01); // Spectate opcode
        self.send_bytes(writer.as_slice())
    }

    /// Send chat message (0x63 + flags + message as UTF-8 for protocol >= 6)
    pub fn send_chat(&self, message: &str) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0x63);
        writer.put_u8(0); // Flags (0 = no reserved bytes)
        writer.put_string_utf8(message);
        self.send_bytes(writer.as_slice())
    }

    /// Send stats request (0xFE) - requests server stats from the server
    pub fn send_stats_request(&self) -> Result<(), JsValue> {
        let mut writer = BinaryWriter::new();
        writer.put_u8(0xFE); // ServerStat opcode
        self.send_bytes(writer.as_slice())
    }
}
