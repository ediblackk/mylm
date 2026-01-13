#!/bin/bash
set -e

echo "🚀 Quick dev install..."

# Check for dependencies
MISSING_DEPS=()
for dep in tmux mold sccache protoc; do
    if ! command -v "$dep" &> /dev/null; then
        MISSING_DEPS+=("$dep")
    fi
done

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo "⚠️  Missing required dev dependencies: ${MISSING_DEPS[*]}"
    echo "   Note: mold and sccache are required by .cargo/config.toml for high-performance builds."
    read -p "Attempt to install them now? (Requires sudo for some) [Y/n]: " install_deps
    if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
        # Check OS
        if [ -f /etc/os-release ]; then
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
        
        # Install sccache via cargo
        if ! command -v sccache &> /dev/null; then
            echo "🚀 Installing sccache via cargo..."
            cargo install sccache
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

# Install binary
sudo cp target/debug/mylm /usr/local/bin/mylm
sudo chmod +x /usr/local/bin/mylm

# Also update /usr/local/bin/ai if it exists to maintain compatibility with existing aliases
if [ -f "/usr/local/bin/ai" ]; then
    sudo cp target/debug/mylm /usr/local/bin/ai
    sudo chmod +x /usr/local/bin/ai
fi

# Verify installation
INSTALLED_VERSION=$(/usr/local/bin/mylm --version)

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
                echo "alias ai='/usr/local/bin/mylm'" >> "$shell_rc"
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
