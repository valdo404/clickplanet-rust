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
SERVICE="click-server"
VERSION="latest"

echo "Building for platform: $PLATFORM"

echo "Building $SERVICE for $PLATFORM..."

docker buildx build \
    --platform $PLATFORM \
    --file Dockerfile.click-server \
    --tag $REGISTRY/$SERVICE:$VERSION \
    --load \
    .

echo "Successfully built $SERVICE"

# List the image
echo "Created image:"
echo "$REGISTRY/$SERVICE:$VERSION"
