#!/bin/bash
set -e

echo "🚀 Quick dev install..."

# Ensure cargo bin is in PATH (sccache lives there)
export PATH="$HOME/.cargo/bin:$PATH"

# Detect OS
OS_TYPE=$(uname -s)

# Define required dependencies based on OS
REQUIRED_DEPS="tmux sccache protoc"
if [ "$OS_TYPE" = "Linux" ]; then
    # mold is primarily for Linux
    REQUIRED_DEPS="$REQUIRED_DEPS mold"
fi

# Check for missing dependencies
MISSING_DEPS=()
for dep in $REQUIRED_DEPS; do
    if ! command -v "$dep" &> /dev/null; then
        MISSING_DEPS+=("$dep")
    fi
done

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo "⚠️  Missing required dev dependencies: ${MISSING_DEPS[*]}"
    echo "   Note: mold and sccache are required by .cargo/config.toml for high-performance builds."
    read -p "Attempt to install them now? [Y/n]: " install_deps
    if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
        if [ "$OS_TYPE" = "Darwin" ]; then
            if command -v brew &> /dev/null; then
                echo "🍎 Detected macOS. Installing dependencies via Homebrew..."
                for dep in "${MISSING_DEPS[@]}"; do
                    PKG_NAME="$dep"
                    # Map binary names to package names if needed
                    if [ "$dep" = "protoc" ]; then PKG_NAME="protobuf"; fi
                    
                    # Skip sccache here if we want to install via cargo, or just use brew
                    # Brew sccache is fine.
                    echo "📦 Installing $PKG_NAME..."
                    brew install "$PKG_NAME"
                done
            else
                echo "❌ Homebrew not found. Please install dependencies manually: ${MISSING_DEPS[*]}"
            fi
        elif [ -f /etc/os-release ]; then
            . /etc/os-release
            case $ID in
                ubuntu|debian|pop|mint)
                    sudo apt-get update && sudo apt-get install -y tmux mold protobuf-compiler
                    ;;
                fedora)
                    sudo dnf install -y tmux mold protobuf-compiler
                    ;;
                arch)
                    sudo pacman -S --noconfirm tmux mold protobuf
                    ;;
            esac
        fi
        
        # Double check sccache if it wasn't installed by system package manager
        echo "🔍 Checking sccache..."
        if ! command -v sccache &> /dev/null; then
            echo "🚀 sccache not found in system packages, installing via cargo..."
            cargo install sccache
            export PATH="$HOME/.cargo/bin:$PATH"
        else
            CURRENT_SCCACHE=$(which sccache 2>/dev/null || echo "not-found")
            echo "ℹ️  sccache found at: $CURRENT_SCCACHE"
        fi
    else
        echo "⚠️  Continuing without dependencies. Build will likely fail."
    fi
fi

# Build the binary
cargo build

# Extract version from the built binary
BUILT_VERSION=$(./target/debug/mylm --version)
echo "📦 Built: $BUILT_VERSION"

# --- Busy Check Function ---
check_busy() {
    local target="$1"
    if [ -f "$target" ]; then
        if command -v fuser &> /dev/null; then
            if fuser "$target" >/dev/null 2>&1; then
                echo "⚠️  Binary $target is currently in use (Text file busy)."
                read -p "Kill running processes using it? [y/N]: " kill_it
                if [[ "$kill_it" =~ ^[Yy]$ ]]; then
                    sudo fuser -k -TERM "$target" >/dev/null 2>&1 || true
                    sleep 0.5
                    if fuser "$target" >/dev/null 2>&1; then
                        sudo fuser -k -KILL "$target" >/dev/null 2>&1 || true
                        sleep 0.5
                    fi
                else
                    echo "❌ Aborting: target file is busy."
                    exit 1
                fi
            fi
        fi
    fi
}

# Determine installation target
# 1. Prefer existing installation in PATH
TARGET_BIN=$(type -P mylm || echo "/usr/local/bin/mylm")

# 2. Filter out the build artifact itself if it happens to be in PATH
if [[ "$TARGET_BIN" == *"target/debug/mylm"* ]]; then
    TARGET_BIN="/usr/local/bin/mylm"
fi

echo "🎯 Installing to: $TARGET_BIN"

# Check permissions
TARGET_DIR=$(dirname "$TARGET_BIN")
SUDO_CMD=""
if [ ! -w "$TARGET_DIR" ] || ( [ -f "$TARGET_BIN" ] && [ ! -w "$TARGET_BIN" ] ); then
    echo "🔒 Elevated permissions required for $TARGET_DIR"
    SUDO_CMD="sudo"
fi

# Ensure directory exists
if [ ! -d "$TARGET_DIR" ]; then
    $SUDO_CMD mkdir -p "$TARGET_DIR"
fi

# Install binary
check_busy "$TARGET_BIN"
$SUDO_CMD cp target/debug/mylm "$TARGET_BIN"
$SUDO_CMD chmod +x "$TARGET_BIN"

# Also update 'ai' if it exists in the same directory (legacy/symlink support)
AI_BIN="${TARGET_DIR}/ai"
if [ -f "$AI_BIN" ]; then
    echo "🔄 Updating legacy alias binary at $AI_BIN..."
    check_busy "$AI_BIN"
    $SUDO_CMD cp target/debug/mylm "$AI_BIN"
    $SUDO_CMD chmod +x "$AI_BIN"
fi

# Verify installation
INSTALLED_VERSION=$("$TARGET_BIN" --version)

if [ "$BUILT_VERSION" == "$INSTALLED_VERSION" ]; then
    echo "✅ Dev binary installed successfully!"
    echo "📌 Version: $INSTALLED_VERSION"

    # --- Tmux Auto-start Configuration (Shared with install.sh) ---
    echo ""
    echo "🔍 Configuring Seamless Terminal Context (tmux auto-start)..."
    shell_rc=""
    if [[ "$SHELL" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "$SHELL" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    fi

    snippet_start="# --- mylm tmux auto-start ---"
    snippet_end="# --- end mylm tmux auto-start ---"

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if grep -q "$snippet_start" "$shell_rc"; then
            if grep -q "tmux attach-session -t mylm" "$shell_rc" || grep -q "tmux new-session -s mylm" "$shell_rc"; then
                echo "⚠️  Found legacy tmux auto-start config that attaches to the shared session 'mylm' (this causes mirroring)."
                echo "🔧 Upgrading to isolated per-terminal tmux sessions..."

                awk -v start="$snippet_start" -v end="$snippet_end" '
                    $0 == start { in_snippet = 1; next }
                    $0 == end   { in_snippet = 0; next }
                    !in_snippet { print }
                ' "$shell_rc" > "${shell_rc}.mylm_tmp" && mv "${shell_rc}.mylm_tmp" "$shell_rc"

                echo "" >> "$shell_rc"
                echo "$snippet_start" >> "$shell_rc"
                echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
                echo '    tmux new-session -s "mylm-$(date +%s)-$$-$RANDOM"' >> "$shell_rc"
                echo 'fi' >> "$shell_rc"
                echo "$snippet_end" >> "$shell_rc"

                echo "✅ Upgraded tmux auto-start snippet in $shell_rc."
                echo "💡 Changes will take effect in new terminal sessions."
            else
                echo "✅ tmux auto-start is already configured in $shell_rc."
            fi
            # Proceed without asking
        else
            read -p "Enable global seamless context via tmux? [y/N]: " enable_tmux
            if [[ "$enable_tmux" =~ ^[Yy]$ ]]; then
                echo "" >> "$shell_rc"
                echo "$snippet_start" >> "$shell_rc"
                echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
                echo '    tmux new-session -s "mylm-$(date +%s)-$$-$RANDOM"' >> "$shell_rc"
                echo 'fi' >> "$shell_rc"
                echo "$snippet_end" >> "$shell_rc"
                echo "✅ Added tmux auto-start snippet to $shell_rc."
                echo "💡 Changes will take effect in new terminal sessions."
            else
                echo "⏭️  Skipping tmux auto-start setup."
            fi
        fi
    else
        echo "⚠️  Could not find shell configuration file to enable tmux auto-start."
    fi

    # --- Shell Alias Configuration ---
    echo ""
    echo "🔍 Configuring shell alias..."
    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if ! grep -q "alias ai=" "$shell_rc"; then
            read -p "Set 'ai' alias in $shell_rc? [y/N]: " set_alias
            if [[ "$set_alias" =~ ^[Yy]$ ]]; then
                echo "alias ai='$TARGET_BIN'" >> "$shell_rc"
                echo "✅ Alias 'ai' added to $shell_rc."
                echo "💡 Please restart your shell or run 'source $shell_rc' to apply changes."
            fi
        else
            echo "✅ Alias 'ai' is already configured in $shell_rc."
        fi
    fi

    echo ""
    echo "� Use 'install.sh' for optimized release builds."
else
    echo "❌ Verification failed!"
    echo "   Built: $BUILT_VERSION"
    echo "   Found: $INSTALLED_VERSION"
    exit 1
fi
