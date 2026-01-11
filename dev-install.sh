#!/bin/bash
set -e

echo "🚀 Quick dev install..."

# Build the binary
cargo build

# Extract version from the built binary
BUILT_VERSION=$(./target/debug/mylm --version)
echo "📦 Built: $BUILT_VERSION"

# Install binary
sudo cp target/debug/mylm /usr/local/bin/mylm
sudo chmod +x /usr/local/bin/mylm

# Verify installation
INSTALLED_VERSION=$(/usr/local/bin/mylm --version)

if [ "$BUILT_VERSION" == "$INSTALLED_VERSION" ]; then
    echo "✅ Dev binary installed successfully!"
    echo "📌 Version: $INSTALLED_VERSION"
    echo "💡 Use 'install.sh' for optimized release builds."
else
    echo "❌ Verification failed!"
    echo "   Built: $BUILT_VERSION"
    echo "   Found: $INSTALLED_VERSION"
    exit 1
fi
