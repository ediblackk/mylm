#!/bin/bash
set -e

# 1. Source Path
SOURCE_BIN="${1:-$MYLM_SOURCE}"

if [ -z "$SOURCE_BIN" ]; then
    echo "Error: Source binary path not provided."
    echo "Usage: $0 <path_to_binary>"
    echo "Or set MYLM_SOURCE environment variable."
    exit 1
fi

if [ ! -f "$SOURCE_BIN" ]; then
    echo "Error: Source file '$SOURCE_BIN' not found."
    exit 1
fi

# 2. Check Root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (sudo) to install to /usr/bin"
    exit 1
fi

echo "Installing mylm from $SOURCE_BIN..."

# 3. Atomic Install
TARGET_BIN="/usr/bin/mylm"
TEMP_BIN="/usr/bin/mylm.new"

# Copy to temp file first
cp "$SOURCE_BIN" "$TEMP_BIN"
chmod +x "$TEMP_BIN"

# Atomic move to replace running binary safely
mv -f "$TEMP_BIN" "$TARGET_BIN"

echo "Installed /usr/bin/mylm"

# 4. Symlink
if [ -L "/usr/bin/ai" ] || [ -f "/usr/bin/ai" ]; then
    rm -f "/usr/bin/ai"
fi
ln -s "$TARGET_BIN" "/usr/bin/ai"
echo "Created symlink /usr/bin/ai -> $TARGET_BIN"

# 5. Clean Legacy
# Determine user home directory safely even under sudo
REAL_USER="${SUDO_USER:-$USER}"

# Try to find home via getent, fallback to HOME if not found or getent missing
if command -v getent >/dev/null; then
    USER_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
else
    # Fallback logic
    if [ "$REAL_USER" = "root" ]; then
        USER_HOME="/root"
    elif [ -d "/home/$REAL_USER" ]; then
        USER_HOME="/home/$REAL_USER"
    else
        USER_HOME="$HOME"
    fi
fi

LEGACY_PATH="$USER_HOME/.local/bin/mylm"
if [ -f "$LEGACY_PATH" ]; then
    echo "Removing legacy installation at $LEGACY_PATH"
    rm -f "$LEGACY_PATH"
fi

# 6. Verify
echo "Verifying installation..."
"$TARGET_BIN" --version

echo "Success!"
