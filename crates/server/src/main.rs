//! Native Ogar game server.

use tracing::info;
use tracing_subscriber::EnvFilter;

mod ai;
mod collision;
mod config;
mod entity;
mod gamemodes;
mod server;
mod spatial;
mod world;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Native Ogar Server v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = config::Config::load()?;
    info!("Loaded configuration");
    info!("  Port: {}", config.server.port);
    info!("  Border: {}x{}", config.border.width, config.border.height);
    info!("  Game mode: {}", config.server.gamemode);

    // Start the game server
    server::run(config).await?;

    Ok(())
}
