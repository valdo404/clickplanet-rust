#!/bin/bash

# Exit on error
set -e

# Determine platform
case $(uname -m) in
    "x86_64")
        PLATFORM="linux/amd64"
        ;;
    "aarch64")
        PLATFORM="linux/arm64"
        ;;
    "arm64")
        PLATFORM="linux/arm64"
        ;;
    *)
        echo "Unsupported architecture: $(uname -m)"
        exit 1
        ;;
esac

REGISTRY="clickplanet"
VERSION="latest"

echo "Building for platform: $PLATFORM"

# Build each service
services=("country-watchguard" "tile-syncer" "click-server" "state-click-persister" )
dockerfiles=("Dockerfile.watchguard-robot" "Dockerfile.tile-syncer-robot" "Dockerfile.click-server" "Dockerfile.click-persister")

for i in "${!services[@]}"; do
    SERVICE=${services[$i]}
    DOCKERFILE=${dockerfiles[$i]}

    echo "Building $SERVICE for $PLATFORM..."

    docker buildx build \
        --platform $PLATFORM \
        --file $DOCKERFILE \
        --tag $REGISTRY/$SERVICE:$VERSION \
        --load \
        .

    echo "Successfully built $SERVICE"
done

echo "All images have been built for $PLATFORM"

# List the images
echo "Created images:"
for SERVICE in "${services[@]}"; do
    echo "$REGISTRY/$SERVICE:$VERSION"
done