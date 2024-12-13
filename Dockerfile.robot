# Build stage
FROM rust:1.83-bookworm AS builder

WORKDIR /usr/src/app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY clickplanet-client/ ./clickplanet-client/
COPY clickplanet-robot/ ./clickplanet-robot/
COPY clickplanet-server/ ./clickplanet-server/
COPY clickplanet-proto/ ./clickplanet-proto/

RUN cargo build --release --bin clickplanet-robot

# Final stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy only the built binary
COPY --from=builder /usr/src/app/target/release/clickplanet-robot ./

# Copy required data files
COPY countries.geojson \
     tile_to_countries.json \
     coordinates.json \
     country_to_tiles.json \
     ./

ENTRYPOINT ["./clickplanet-robot"]
CMD ["--target-country", "ru", "--wanted-country", "ru", "--port", "3000", "--host", "localhost", "--unsecure"]