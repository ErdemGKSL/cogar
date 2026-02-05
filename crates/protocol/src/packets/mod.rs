//! Packet definitions for the Ogar protocol.
//!
//! This module contains both client->server and server->client packet types.

mod client;
mod server;

pub use client::*;
pub use server::*;

/// Opcodes for client -> server packets.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientOpcode {
    /// Join game with nickname.
    Join = 0x00,
    /// Request spectate mode.
    Spectate = 0x01,
    /// Mouse position update.
    Mouse = 0x10,
    /// Split (Space key).
    Split = 0x11,
    /// Q key (minion toggle).
    KeyQ = 0x12,
    /// Eject mass (W key).
    Eject = 0x15,
    /// E key (minion split).
    KeyE = 0x16,
    /// R key (minion eject).
    KeyR = 0x17,
    /// T key (minion freeze).
    KeyT = 0x18,
    /// P key (minion collect).
    KeyP = 0x19,
    /// Chat message.
    Chat = 0x63,
    /// Protocol version handshake.
    Protocol = 0xFE,
    /// Handshake key.
    HandshakeKey = 0xFF,
}

/// Opcodes for server -> client packets.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerOpcode {
    /// World update (add/update/remove nodes).
    UpdateNodes = 0x10,
    /// Spectator position update.
    UpdatePosition = 0x11,
    /// Clear all nodes.
    ClearAll = 0x12,
    /// Clear owned cells.
    ClearOwned = 0x14,
    /// Add owned node.
    AddNode = 0x20,
    /// Leaderboard (text list).
    LeaderboardText = 0x30,
    /// Leaderboard (FFA).
    LeaderboardFFA = 0x31,
    /// Leaderboard (teams/pie chart).
    LeaderboardPie = 0x32,
    /// Set world border.
    SetBorder = 0x40,
    /// Xray data (operator only).
    XrayData = 0x50,
    /// Chat message.
    ChatMessage = 0x63,
    /// Server stats (ping response).
    ServerStat = 0xFE,
}
