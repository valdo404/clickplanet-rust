# ClickPlanet Client

A Rust client implementation for interacting with [ClickPlanet](https://clickplanet.lol), a multiplayer geographic clicking game.

## Protocol

The game uses Protocol Buffers for all communication

## Building

Requires Rust and Cargo. To build:

```bash
cargo build
```

## API Endpoints

- WebSocket: wss://clickplanet.lol/ws/listen
- Click: POST https://clickplanet.lol/api/click
- Ownerships: GET https://clickplanet.lol/api/ownerships
- Batch Ownerships: POST https://clickplanet.lol/api/ownerships-by-batch

## Dependencies

- prost: Protocol Buffers implementation
- tokio-tungstenite: WebSocket client
- tokio: Async runtime
- native-tls: TLS support