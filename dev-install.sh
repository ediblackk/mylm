#!/bin/bash
set -e

echo "🚀 Quick dev install..."

# Check for tmux (prerequisite for Pop Terminal)
if ! command -v tmux &> /dev/null; then
    echo "⚠️  Warning: tmux is not installed. The 'Pop Terminal' feature requires it."
    echo "   You can install it with: sudo apt install tmux (Linux) or brew install tmux (macOS)"
    read -p "Continue anyway? [y/N]: " continue_anyway
    if [[ ! "$continue_anyway" =~ ^[Yy]$ ]]; then
        exit 1
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

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if grep -q "mylm tmux auto-start" "$shell_rc"; then
            echo "✅ tmux auto-start is already configured in $shell_rc."
            # Proceed without asking
        else
            read -p "Enable global seamless context via tmux? [y/N]: " enable_tmux
            if [[ "$enable_tmux" =~ ^[Yy]$ ]]; then
                echo "" >> "$shell_rc"
                echo "# --- mylm tmux auto-start ---" >> "$shell_rc"
                echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
                echo '    tmux attach-session -t mylm 2>/dev/null || tmux new-session -s mylm' >> "$shell_rc"
                echo 'fi' >> "$shell_rc"
                echo "# --- end mylm tmux auto-start ---" >> "$shell_rc"
                echo "✅ Added tmux auto-start snippet to $shell_rc."
                echo "💡 Changes will take effect in new terminal sessions."
            else
                echo "⏭️  Skipping tmux auto-start setup."
            fi
        fi
    else
        echo "⚠️  Could not find shell configuration file to enable tmux auto-start."
    fi

    echo ""
    echo "� Use 'install.sh' for optimized release builds."
else
    echo "❌ Verification failed!"
    echo "   Built: $BUILT_VERSION"
    echo "   Found: $INSTALLED_VERSION"
    exit 1
fi
