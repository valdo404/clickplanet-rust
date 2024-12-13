# ClickPlanet Project

A Rust client implementation for interacting with [ClickPlanet](https://clickplanet.lol), a multiplayer geographic clicking game.

## Protocol

The game uses Protocol Buffers for all communication

## Features

- A country safeguard robot.
- A REST / Websocket client
- A server using CQRS architecture.
 - Stack: hyper / axium
 - Redis for cold state recovery
 - NATS for N-to-N click exchanges with full in-memory management

# Perfs
```
(base) ➜  clickplanet-client git:(feat-server) ✗ time curl 'http://localhost:3000/v2/rpc/leaderboard' \
  -H 'accept: */*' \
  -H 'accept-language: en-GB,en;q=0.9,fr-FR;q=0.8,fr;q=0.7,en-US;q=0.6' \
  -H 'content-type: application/json' \
  -H 'origin: https://clickplanet.lol' \
  -H 'priority: u=1, i' \
  -H 'referer: https://clickplanet.lol/' \
  -H 'sec-ch-ua: "Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "macOS"' \
  -H 'sec-fetch-dest: empty' \
  -H 'sec-fetch-mode: cors' \
  -H 'sec-fetch-site: same-origin' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36'|jq -r .data|base64 -d|protobuf_inspector
  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
                                 Dload  Upload   Total   Spent    Left  Speed
100    75  100    75    0     0   2587      0 --:--:-- --:--:-- --:--:--  2678
root:
    1 <chunk> = message(1 <chunk> = "ru", 2 <varint> = 31473)
    1 <chunk> = message(1 <chunk> = "fr", 2 <varint> = 16478)
    1 <chunk> = message(1 <chunk> = "es", 2 <varint> = 874)
    1 <chunk> = message(1 <chunk> = "sr", 2 <varint> = 544)
    1 <chunk> = message:
        1 <chunk> = message:
            14 <message> = group (end 12) message()
        2 <varint> = 1

curl 'http://localhost:3000/v2/rpc/leaderboard' -H 'accept: */*' -H  -H  -H    0.01s user 0.01s system 37% cpu 0.055 total
jq -r .data  0.03s user 0.00s system 58% cpu 0.054 total
base64 -d  0.00s user 0.00s system 8% cpu 0.053 total
protobuf_inspector  0.03s user 0.01s system 71% cpu 0.053 total
```

Small benchmark.

```
cargo run --bin clickplanet-robot -- --target-country ru --wanted-country ru --port 3000 --host localhost --unsecure
cargo run --bin clickplanet-robot -- --target-country ru --wanted-country fr --port 3000 --host localhost --unsecure
```

This generates gygabytes of events, and despite all of this, the websocket endpoints do not saturate

## Deployment

Will be possibly be done using cloud run and/or GKE and/or EKS. Or maybe just a simple, minikube tiny server.
It needs:
- 1-N NATS server / cluster
- 1-N redis server / cluster
- 1-N worker for redis storage
- 1-N servers


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
