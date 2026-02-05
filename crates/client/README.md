# native-agar-client

Rust + WebAssembly client for native-agar. This is a systematic, modular port of the Cigar2 client.

## Architecture

The client is organized into focused modules with clear separation of concerns:

- **`network/`** — WebSocket connection, binary protocol handling, scrambling
- **`game/`** — Game state, cell management, world representation
- **`render/`** — Canvas 2D rendering (grid, cells, names, skins, UI)
- **`camera/`** — Viewport transformation, zoom, smooth follow
- **`input/`** — Mouse and keyboard event handling
- **`ui/`** — DOM manipulation, overlays, menus, chat
- **`utils/`** — Helper functions (LERP, math, logging)

## Building

```bash
# Install wasm-pack if not already installed
cargo install wasm-pack

# Build the WASM module
cd crates/client
wasm-pack build --target web --out-dir ../../web/pkg

# Serve with a local web server
# (TODO: Add build pipeline with Vite or similar)
```

## Development Philosophy

**The logic and behavior must exactly match the JS reference client** (`js-source/client/web/assets/js/main_out.js`), but the code organization follows idiomatic Rust practices:

- Single responsibility per module
- Clean public APIs, private implementation details
- Rust idioms (Result, Option, iterators, traits)
- Type safety and compile-time guarantees

**Do not replicate the monolithic JS structure.** Each feature should be in its appropriate module with proper boundaries.

## Status

🚧 **In Progress** — Basic structure initialized, implementation pending.

See [tasks/client.md](../../tasks/client.md) for the full implementation checklist.
