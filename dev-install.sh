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

    echo ""
    echo "� Use 'install.sh' for optimized release builds."
else
    echo "❌ Verification failed!"
    echo "   Built: $BUILT_VERSION"
    echo "   Found: $INSTALLED_VERSION"
    exit 1
fi
