#!/bin/bash
set -e

APP="terminaldrome"
VERSION="0.2.3"
DIST="dist"

mkdir -p "$DIST"

declare -A TARGETS=(
    ["x86_64-apple-darwin"]="macOS-Intel"
    ["aarch64-apple-darwin"]="macOS-ARM64"
    ["x86_64-unknown-linux-gnu"]="Linux-x86_64"
    ["aarch64-unknown-linux-gnu"]="Linux-aarch64"
    ["powerpc64-unknown-linux-gnu"]="Linux-PPC64"
)

for TARGET in "${!TARGETS[@]}"; do
    LABEL="${TARGETS[$TARGET]}"
    echo "Building for $LABEL ($TARGET)..."
    
    cross build --release --target "$TARGET"
    
    BINARY="target/$TARGET/release/$APP"
    if [ -f "$BINARY" ]; then
        cp "$BINARY" "$DIST/${APP}-${VERSION}-${LABEL}"
        echo "  ✓ $DIST/${APP}-${VERSION}-${LABEL}"
    else
        echo "  ✗ Build failed for $TARGET"
    fi
done

echo ""
echo "Built binaries:"
ls -lh "$DIST/"
