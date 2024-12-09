# ClickPlanet Project

A Rust client implementation for interacting with [ClickPlanet](https://clickplanet.lol), a multiplayer geographic clicking game.

## Protocol

The game uses Protocol Buffers for all communication

## Features

- A country safeguard robot.
- A REST / Websocket client
 - TODO fix the websocket client (seems broken)
 - TODO Rely on http pools. Fix tokio concurrency
- A server (TODO), using CQRS architecture.
 - Expected Stack: hyper / axium / redis for cold state recovery / Actix for in fast in memory states / capnproto or grpc for write / read synchronisation / redstar for offline analysis. 

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
