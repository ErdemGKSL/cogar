//! Ogar - Pure game server binary

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Ogar - Pure Game Server v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = server::Config::load()?;
    info!("Loaded configuration");
    info!("  Port: {}", config.server.port);
    info!("  Border: {}x{}", config.border.width, config.border.height);
    info!("  Game mode: {}", config.server.gamemode);

    // Start the pure game server (WebSocket only)
    server::run(config).await?;

    Ok(())
}