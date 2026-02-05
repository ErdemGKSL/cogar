//! Protocol error types.

use thiserror::Error;

/// Errors that can occur during protocol parsing.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Invalid packet opcode: {0:#04x}")]
    InvalidOpcode(u8),

    #[error("Unexpected end of data")]
    UnexpectedEof,

    #[error("Unsupported protocol version: {0}")]
    UnsupportedProtocol(u32),

    #[error("Invalid handshake key")]
    InvalidHandshakeKey,
}
