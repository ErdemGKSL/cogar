//! Cigar - Static frontend serving binary

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::OnceLock;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

// Embedded static assets from client/web
#[derive(RustEmbed)]
#[folder = "../client/web"]
struct Assets;

// Cache for generated skins list
static SKINS_LIST: OnceLock<String> = OnceLock::new();

/// Generate comma-separated list of available skins from embedded assets
fn generate_skins_list() -> String {
    let mut skins = Vec::new();
    
    // Iterate through all embedded assets
    for file_path in Assets::iter() {
        // Check if file is in skins/ directory
        if file_path.starts_with("skins/") {
            // Extract filename without extension
            if let Some(filename) = file_path.strip_prefix("skins/") {
                // Remove .png or .webp extension
                let skin_name = filename
                    .strip_suffix(".png")
                    .or_else(|| filename.strip_suffix(".webp"))
                    .unwrap_or(filename);
                
                // Avoid duplicates (same skin might have both .png and .webp)
                if !skins.contains(&skin_name.to_string()) {
                    skins.push(skin_name.to_string());
                }
            }
        }
    }
    
    // Sort alphabetically for consistency
    skins.sort();
    
    // Join with commas
    skins.join(",")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Cigar - Frontend Static Server v{}", env!("CARGO_PKG_VERSION"));

    // Default port for static serving
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    // Generate skins list at startup
    let skins_list = generate_skins_list();
    info!("Found {} skins: {}", skins_list.split(',').filter(|s| !s.is_empty()).count(), skins_list);
    SKINS_LIST.set(skins_list).ok();

    // Build the axum router for static file serving only
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/skinList.txt", get(serve_skins_txt))
        .fallback(static_handler)
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()));

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Frontend server running on http://{}", addr);

    axum::serve(listener, app.into_make_service())
        .await?;

    Ok(())
}

/// Serve the main index.html page
async fn serve_index() -> impl IntoResponse {
    serve_static_file("index.html".to_string()).await
}

/// Serve dynamically generated skins.txt
async fn serve_skins_txt() -> impl IntoResponse {
    let skins_list = SKINS_LIST.get().cloned().unwrap_or_default();
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(axum::body::Body::from(skins_list))
        .unwrap()
}

/// Handle static file requests
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/').to_string();

    // Handle empty path or root
    if path.is_empty() || path == "/" {
        return serve_static_file("index.html".to_string()).await;
    }

    serve_static_file(path).await
}

/// Serve a static file from embedded assets
async fn serve_static_file(path: String) -> impl IntoResponse {
    match Assets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            
            // For main.js, inject connection URLs from environment
            let body = if path == "main.js" {
                if let Ok(content_str) = std::str::from_utf8(&content.data) {
                    // Parse CONNECT_TO environment variable
                    let connection_config = if let Ok(connect_to) = std::env::var("CONNECT_TO") {
                        parse_connection_string(&connect_to)
                    } else {
                        "[]".to_string() // Empty array if no env var
                    };
                    
                    let injected_content = content_str.replace(
                        "// CIGAR_CONNECTION_INJECT_POINT",
                        &format!("window.CIGAR_CONNECTIONS = {}; // Auto-injected by cigar", connection_config)
                    );
                    
                    axum::body::Body::from(injected_content)
                } else {
                    axum::body::Body::from(content.data.to_vec())
                }
            } else {
                axum::body::Body::from(content.data.to_vec())
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(body)
                .unwrap()
        }
        None => {
            warn!("Static file not found: {}", path);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap()
        }
    }
}

// Parse CONNECT_TO environment variable format
// Format: "100.0.0.0;Server1,/game;Local Game,ws://other.com/ws;Remote"
fn parse_connection_string(connect_to: &str) -> String {
    let mut connections = Vec::new();
    
    for entry in connect_to.split(',') {
        let parts: Vec<&str> = entry.split(';').collect();
        if parts.is_empty() {
            continue;
        }
        
        let url = parts[0].trim();
        let name = if parts.len() > 1 && !parts[1].trim().is_empty() {
            parts[1].trim().to_string()
        } else {
            url.to_string()
        };
        
        connections.push(format!(
            "{{\"url\": \"{}\", \"name\": \"{}\"}}",
            url, name
        ));
    }
    
    format!("[{}]", connections.join(","))
}