//! Native Ogar game server library.

pub mod ai;
pub mod collision;
pub mod config;
pub mod entity;
pub mod gamemodes;
pub mod server;
pub mod spatial;
pub mod world;

// Re-export commonly used types
pub use config::Config;
pub use server::{
    run, ChatBroadcast, LeaderboardBroadcast, WorldUpdateBroadcast, TargetedMessage, TargetedMessageType,
    ClientViewData, WorldCell
};