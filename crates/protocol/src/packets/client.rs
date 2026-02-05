//! Client -> Server packet parsing.

use crate::{BinaryReader, ProtocolError};

/// Parsed client packet.
#[derive(Debug, Clone)]
pub enum ClientPacket {
    /// Protocol version (0xFE).
    Protocol(u32),
    /// Handshake key (0xFF).
    HandshakeKey(u32),
    /// Join game (0x00) with nickname.
    Join { name: String },
    /// Spectate mode (0x01).
    Spectate,
    /// Mouse position (0x10).
    Mouse { x: i32, y: i32 },
    /// Split (0x11).
    Split,
    /// Q key (0x12).
    KeyQ,
    /// Eject (0x15).
    Eject,
    /// E key (0x16).
    KeyE,
    /// R key (0x17).
    KeyR,
    /// T key (0x18).
    KeyT,
    /// P key (0x19).
    KeyP,
    /// Chat message (0x63).
    Chat { flags: u8, message: String },
    /// Stats request (0xFE with len=1).
    StatsRequest,
}

impl ClientPacket {
    /// Parse a client packet from raw bytes.
    ///
    /// `protocol` is the negotiated protocol version (used for string encoding).
    pub fn parse(data: &[u8], protocol: u32) -> Result<Self, ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::UnexpectedEof);
        }

        let mut reader = BinaryReader::new(data.to_vec());
        let opcode = reader.get_u8();

        match opcode {
            0xFE => {
                if data.len() == 1 {
                    // Stats request
                    Ok(ClientPacket::StatsRequest)
                } else if data.len() == 5 {
                    // Protocol version
                    let version = reader.get_u32();
                    Ok(ClientPacket::Protocol(version))
                } else {
                    Err(ProtocolError::InvalidOpcode(opcode))
                }
            }
            0xFF => {
                if data.len() != 5 {
                    return Err(ProtocolError::InvalidHandshakeKey);
                }
                let key = reader.get_u32();
                Ok(ClientPacket::HandshakeKey(key))
            }
            0x00 => {
                // Join
                let name = if protocol > 6 {
                    reader.get_string_unicode()
                } else {
                    reader.get_string_utf8()
                };
                Ok(ClientPacket::Join { name })
            }
            0x01 => Ok(ClientPacket::Spectate),
            0x10 => {
                // Mouse - supports multiple formats
                let (x, y) = match data.len() {
                    13 => {
                        let x = reader.get_i32();
                        let y = reader.get_i32();
                        (x, y)
                    }
                    9 => {
                        let x = reader.get_i16() as i32;
                        let y = reader.get_i16() as i32;
                        (x, y)
                    }
                    21 => {
                        let x = reader.get_f64() as i32;
                        let y = reader.get_f64() as i32;
                        (x, y)
                    }
                    _ => return Err(ProtocolError::InvalidOpcode(opcode)),
                };
                Ok(ClientPacket::Mouse { x, y })
            }
            0x11 => Ok(ClientPacket::Split),
            0x12 => Ok(ClientPacket::KeyQ),
            0x15 => Ok(ClientPacket::Eject),
            0x16 => Ok(ClientPacket::KeyE),
            0x17 => Ok(ClientPacket::KeyR),
            0x18 => Ok(ClientPacket::KeyT),
            0x19 => Ok(ClientPacket::KeyP),
            0x63 => {
                // Chat
                if data.len() < 3 {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let flags = reader.get_u8();
                // Skip reserved bytes based on flags
                let rv_len = if flags & 2 != 0 { 4 } else { 0 }
                    + if flags & 4 != 0 { 8 } else { 0 }
                    + if flags & 8 != 0 { 16 } else { 0 };
                reader.skip(rv_len);

                let message = if protocol < 6 {
                    reader.get_string_unicode()
                } else {
                    reader.get_string_utf8()
                };
                Ok(ClientPacket::Chat { flags, message })
            }
            _ => Err(ProtocolError::InvalidOpcode(opcode)),
        }
    }
}
