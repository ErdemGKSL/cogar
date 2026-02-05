# Cogar.rs

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org/)
[![Wasm](https://img.shields.io/badge/wasm-compatible-purple.svg)](https://webassembly.org/)

**Cogar** is a high-performance, unified game server and client for Agar.io-style game, built from the ground up in Rust. It aims to provide a modern, efficient, and easily deployable alternative to legacy Ogar implementations.

## 🚀 Key Features

-   **Unified Binary:** A single executable (`cogar`) that serves both the high-performance game logic and the embedded web-based client.
-   **High Performance:** Optimized game loop and spatial partitioning implemented in Rust for low latency and high player counts.
-   **Protocol Compatibility:** Implements the standard Ogar protocol, allowing compatibility with various **Cigar**-based clients.
-   **Interoperability:** The **Cigar** client can be used to connect to any standard **Ogar** server, and vice versa.
-   **Wasm-Powered Client:** The client is built using Rust and compiled to WebAssembly, ensuring smooth rendering and shared logic between server and client.
-   **Modern Tech Stack:** Utilizes `axum` for the web server, `tokio` for asynchronous networking, and `Tailwind CSS` for a premium UI aesthetics.
-   **Advanced AI:** Integrated bot manager and players with customizable behaviors.
-   **Multiple Gamemodes:** Supports various classic and experimental modes:
    -   FFA (Free For All)
    -   Teams
    -   Hunger Games
    -   Tournament
    -   Beatdown
    -   Experimental
    -   Rainbow

## 🛠️ Tech Stack

-   **Backend:** [Rust](https://www.rust-lang.org/) (axum, tokio, tungstenite, serde, glam)
-   **Frontend:** Rust (Wasm), JavaScript, CSS ([Tailwind CSS 4.0](https://tailwindcss.com/))
-   **Assets:** Embedded directly into the binary using `rust-embed`.
-   **Communication:** WebSockets with a custom binary protocol optimized for speed.

## 📦 Project Structure

```text
.
├── crates/
│   ├── bin/        # Entry points (cogar, cigar, ogar)
│   ├── client/     # Wasm client and web assets
│   ├── protocol/   # Shared binary protocol definitions
│   └── server/     # Core game logic, physics, and gamemodes
├── config.toml     # Server configuration
└── Cargo.toml      # Workspace management
```

## 🚥 Getting Started

### Prerequisites

-   [Rust](https://www.rust-lang.org/tools/install) (latest stable or nightly)
-   [Wasm-bindgen-cli](https://rustwasm.github.io/wasm-bindgen/reference/cli.html) (for client compilation)
-   [Node.js / Bun](https://nodejs.org/ / https://bun.sh/) (for frontend asset processing)

### Running the Project

You can choose between the unified experience or standalone components:

- **Unified (Server + Client):**
  ```bash
  cargo run --release --bin cogar
  ```
- **Pure Game Server (Ogar):**
  ```bash
  cargo run --release --bin ogar
  ```
- **Standalone Frontend Server (Cigar):**
  ```bash
  cargo run --release --bin cigar
  ```

The unified server (`cogar`) will be available at `http://localhost:3000` by default.

## ⚙️ Configuration

The server can be configured via `config.toml` in the root directory. You can adjust:

-   Network settings (Host, Port)
-   Game mechanics (Map size, cell speeds, decay rates)
-   Bot settings
-   Gamemode specific parameters

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request or open an issue for bugs and feature requests.

## 📄 License

This project is licensed under the Apache License 2.0. See the [LICENSE](LICENSE) file for details.
