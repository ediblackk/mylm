#!/bin/bash

# Exit on error
set -e

# Configuration Setup
CONFIG_DIR="$HOME/.config/mylm"
CONFIG_FILE="$CONFIG_DIR/mylm.yaml"
BINARY_DEST="/usr/local/bin/mylm"

# --- Utility Functions ---

get_current_version() {
    if [ -f "Cargo.toml" ]; then
        grep '^version =' Cargo.toml | cut -d '"' -f 2
    else
        echo "unknown"
    fi
}

get_installed_version() {
    if [ -f "$BINARY_DEST" ]; then
        "$BINARY_DEST" --version 2>/dev/null | awk '{print $2}' || echo "none"
    else
        echo "none"
    fi
}

check_and_install_dependencies() {
    echo "🔍 Checking for system dependencies..."
    
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS=$ID
    else
        OS=$(uname -s)
    fi

    MISSING_DEPS=()
    
    case $OS in
        ubuntu|debian|pop|mint)
            # Core build tools and libraries for mylm (OpenSSL, XCB, etc.) + tmux for terminal context
            DEPS=("pkg-config" "libssl-dev" "libxcb1-dev" "libxcb-render0-dev" "libxcb-shape0-dev" "libxcb-xfixes0-dev" "clang" "build-essential" "cmake" "tmux")
            for dep in "${DEPS[@]}"; do
                if ! dpkg -l | grep -qw "$dep" &>/dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                echo "⚠️  Missing dependencies: ${MISSING_DEPS[*]}"
                read -p "Would you like to install them now? (Requires sudo) [Y/n]: " install_deps
                if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
                    sudo apt-get update
                    sudo apt-get install -y "${MISSING_DEPS[@]}"
                fi
            fi
            ;;
        fedora)
            DEPS=("pkgconf-pkg-config" "openssl-devel" "libxcb-devel" "clang" "gcc-c++" "cmake" "tmux")
            for dep in "${DEPS[@]}"; do
                if ! rpm -q "$dep" &> /dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                echo "⚠️  Missing dependencies: ${MISSING_DEPS[*]}"
                read -p "Would you like to install them now? (Requires sudo) [Y/n]: " install_deps
                if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
                    sudo dnf install -y "${MISSING_DEPS[@]}"
                fi
            fi
            ;;
        arch)
            DEPS=("pkgconf" "openssl" "libxcb" "clang" "base-devel" "cmake" "tmux")
            for dep in "${DEPS[@]}"; do
                if ! pacman -Qs "$dep" &> /dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                echo "⚠️  Missing dependencies: ${MISSING_DEPS[*]}"
                read -p "Would you like to install them now? (Requires sudo) [Y/n]: " install_deps
                if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
                    sudo pacman -S --noconfirm "${MISSING_DEPS[@]}"
                fi
            fi
            ;;
        Darwin)
            # macOS dependencies via Homebrew
            if command -v brew &> /dev/null; then
                DEPS=("openssl" "pkg-config" "tmux")
                for dep in "${DEPS[@]}"; do
                    if ! brew list "$dep" &> /dev/null; then
                        MISSING_DEPS+=("$dep")
                    fi
                done
                if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                    echo "⚠️  Missing dependencies: ${MISSING_DEPS[*]}"
                    read -p "Would you like to install them now? [Y/n]: " install_deps
                    if [[ ! "$install_deps" =~ ^[Nn]$ ]]; then
                        brew install "${MISSING_DEPS[@]}"
                    fi
                fi
            else
                echo "⚠️  Homebrew not found. Please ensure you have 'openssl', 'pkg-config', and 'tmux' installed manually."
            fi
            ;;
        *)
            echo "⚠️  Unsupported or unknown OS: $OS. Please ensure you have the required build tools installed manually."
            ;;
    esac

    # Optional: sccache for faster builds
    if ! command -v sccache &> /dev/null; then
        read -p "Would you like to install sccache to speed up future builds? [y/N]: " install_sccache
        if [[ "$install_sccache" =~ ^[Yy]$ ]]; then
            cargo install sccache || echo "⚠️  Failed to install sccache via cargo, skipping."
        fi
    fi

    # Check for Rust/Cargo
    if ! command -v cargo &> /dev/null; then
        echo "❌ Rust/Cargo not found."
        read -p "Would you like to install Rust now via rustup.rs? [Y/n]: " install_rust
        if [[ ! "$install_rust" =~ ^[Nn]$ ]]; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source "$HOME/.cargo/env"
        else
            echo "❌ Error: Rust is required to build mylm. Exiting."
            exit 1
        fi
    fi
}

build_binary() {
    local force_rebuild=$1
    local profile=${2:-"release"}  # Default release, dar poate fi schimbat
    
    if [ "$force_rebuild" != "true" ] && [ -f "./target/$profile/mylm" ]; then
        echo "✨ Found an existing $profile binary at ./target/$profile/mylm."
        read -p "Would you like to rebuild it to ensure it's the latest version? [y/N]: " rebuild
        if [[ ! "$rebuild" =~ ^[Yy]$ ]]; then
            echo "⏭️  Skipping build, using existing binary."
            return 0
        fi
    fi

    echo "🚀 Building mylm in $profile mode..."
    
    # Întreabă utilizatorul ce profil vrea
    if [ "$profile" == "release" ]; then
        read -p "Use optimized release build (20 min) or fast dev build (7 min)? [r/D]: " build_type
        if [[ "$build_type" =~ ^[Rr]$ ]]; then
            profile="release"
        else
            profile="debug"
        fi
    fi
    
    if [ "$profile" == "release" ]; then
        if command -v sccache &> /dev/null; then
            RUSTC_WRAPPER=sccache cargo build --release
        else
            cargo build --release
        fi
    else
        if command -v sccache &> /dev/null; then
            RUSTC_WRAPPER=sccache cargo build
        else
            cargo build
        fi
    fi
}

install_binary() {
    local profile=${1:-"release"}
    echo "📦 Installing/Updating binary to $BINARY_DEST..."
    sudo cp target/$profile/mylm "$BINARY_DEST"
    sudo chmod +x "$BINARY_DEST"
    
    # Also update /usr/local/bin/ai if it exists to maintain compatibility
    if [ -f "/usr/local/bin/ai" ]; then
        sudo cp target/$profile/mylm /usr/local/bin/ai
        sudo chmod +x /usr/local/bin/ai
    fi
    
    echo "✅ Binary installed successfully."
}

setup_shell_alias() {
    local mandatory=$1
    echo ""
    echo "🔍 Configuring shell alias..."
    
    local shell_rc=""
    if [[ "$SHELL" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "$SHELL" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    fi

    if [ -n "$shell_rc" ]; then
        local chosen_alias
        read -p "Set your preferred alias to call mylm [default: ai] : " chosen_alias
        chosen_alias="${chosen_alias:-ai}"

        # Basic validation: no spaces
        if [[ "$chosen_alias" =~ [[:space:]] ]]; then
            echo "❌ Alias cannot contain spaces. Falling back to 'ai'."
            chosen_alias="ai"
        fi

        # Check for conflicts with existing commands (that aren't our own alias)
        if command -v "$chosen_alias" &> /dev/null && ! grep -q "alias $chosen_alias=" "$shell_rc"; then
            echo "⚠️  Warning: '$chosen_alias' already exists as a command or alias."
            read -p "Are you sure you want to use '$chosen_alias'? [y/N]: " confirm_conflict
            if [[ ! "$confirm_conflict" =~ ^[Yy]$ ]]; then
                echo "⏭️  Skipping alias setup."
                return 0
            fi
        fi

        if grep -q "alias $chosen_alias=" "$shell_rc"; then
            echo "⚠️  Found an existing '$chosen_alias' alias in $shell_rc."
            if [ "$mandatory" == "true" ]; then
                sed -i "/alias $chosen_alias=/d" "$shell_rc"
                echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
                echo "✅ Alias updated in $shell_rc."
            else
                read -p "Would you like to replace it? [y/N]: " replace_alias
                if [[ "$replace_alias" =~ ^[Yy]$ ]]; then
                    sed -i "/alias $chosen_alias=/d" "$shell_rc"
                    echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
                    echo "✅ Alias updated in $shell_rc."
                fi
            fi
        else
            echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
            echo "✅ Alias '$chosen_alias' added to $shell_rc."
        fi
        echo "💡 Please restart your shell or run 'source $shell_rc' to apply changes."
    else
        echo "⚠️  Could not determine your shell configuration file. Please manually add: alias ai='$BINARY_DEST'"
    fi
}

setup_tmux_autostart() {
    echo ""
    echo "🔍 Configuring Seamless Terminal Context (tmux auto-start)..."
    echo "💡 This is the secret to a 'magical' terminal evolution experience."
    echo "   By auto-starting tmux, every command you run and every output you see"
    echo "   is instantly accessible to the AI when you run 'ai pop'."
    echo ""
    echo "   - It only attaches if you aren't already in tmux."
    echo "   - It keeps your session alive if the terminal closes."
    echo "   - It's the only way to capture full scrollback history seamlessly."
    
    local shell_rc=""
    if [[ "$SHELL" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "$SHELL" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    fi

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if grep -q "mylm tmux auto-start" "$shell_rc"; then
            echo "✅ tmux auto-start is already configured in $shell_rc."
            return 0
        fi
    fi

    read -p "Enable global seamless context via tmux? [y/N]: " enable_tmux
    if [[ ! "$enable_tmux" =~ ^[Yy]$ ]]; then
        echo "⏭️  Skipping tmux auto-start setup."
        return 0
    fi

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
            echo "" >> "$shell_rc"
            echo "# --- mylm tmux auto-start ---" >> "$shell_rc"
            echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
            echo '    tmux attach-session -t mylm 2>/dev/null || tmux new-session -s mylm' >> "$shell_rc"
            echo 'fi' >> "$shell_rc"
            echo "# --- end mylm tmux auto-start ---" >> "$shell_rc"
            echo "✅ Added tmux auto-start snippet to $shell_rc."
            echo "💡 Changes will take effect in new terminal sessions."
        fi
    else
        echo "⚠️  Could not find shell configuration file to enable tmux auto-start."
    fi
}

run_setup() {
    local mandatory=$1
    echo ""
    echo "⚙️  Running Configuration Setup..."
    
    if [ "$mandatory" == "true" ]; then
        "$BINARY_DEST" setup
    else
        read -p "Would you like to run the configuration wizard (setup)? [y/N]: " launch_setup
        if [[ "$launch_setup" =~ ^[Yy]$ ]]; then
            "$BINARY_DEST" setup
        fi
    fi
}

# --- Main Flow Functions ---

full_installation() {
    echo "🌟 Starting Fresh Installation..."
    check_and_install_dependencies
    
    # Only clean if user explicitly wants it
    read -p "Would you like to clean previous build artifacts? (Forces full rebuild) [y/N]: " do_clean
    if [[ "$do_clean" =~ ^[Yy]$ ]]; then
        echo "🧹 Cleaning previous build artifacts..."
        cargo clean
    fi
    
    # Temporary: Use dev-install.sh for faster iteration during development phase
    echo "⚠️  Using dev-install.sh for development build..."
    if [ -f "./dev-install.sh" ]; then
        chmod +x dev-install.sh
        ./dev-install.sh
    else
        echo "❌ dev-install.sh not found, falling back to standard build..."
        build_binary "true"
        install_binary
    fi
    setup_shell_alias "true"
    setup_tmux_autostart
    run_setup "true"
    
    echo ""
    echo "✅ Fresh installation complete!"
}

update_existing() {
    echo "🔄 Checking for updates..."
    local current=$(get_current_version)
    local installed=$(get_installed_version)
    
    echo "📦 Local Source Version: $current"
    echo "📦 Installed Binary Version: $installed"
    
    if [ "$current" == "$installed" ]; then
        echo "✨ You already have the latest version installed ($installed)."
        read -p "Force rebuild and reinstall anyway? [y/N]: " force_update
        if [[ ! "$force_update" =~ ^[Yy]$ ]]; then
            return 0
        fi
    else
        echo "🆕 A different version is available. Updating..."
    fi

    check_and_install_dependencies
    
    # Temporary: Use dev-install.sh for faster iteration during development phase
    echo "⚠️  Using dev-install.sh for development build..."
    if [ -f "./dev-install.sh" ]; then
        chmod +x dev-install.sh
        ./dev-install.sh
    else
        echo "❌ dev-install.sh not found, falling back to standard build..."
        build_binary "false"
        install_binary
    fi
    
    echo ""
    echo "✅ Update complete! (Your configuration and aliases were preserved)"
}

show_menu() {
    local current=$(get_current_version)
    local installed=$(get_installed_version)
    
    echo "------------------------------------------------"
    echo "   🤖 mylm Installation & Setup Wizard v$current   "
    echo "------------------------------------------------"
    echo "Status: Installed v$installed"
    echo "------------------------------------------------"
    echo "1) 🚀 Fresh Installation (Full Wipe & Setup)"
    echo "2) 🔄 Update Existing (Build & Update Binary Only)"
    echo "3) 🔗 Setup Shell Alias Only"
    echo "4) ⚙️  Run Configuration Wizard (setup)"
    echo "5) ❌ Exit"
    echo "------------------------------------------------"
}

# --- Main Loop ---

while true; do
    show_menu
    read -p "Select an option [1-5]: " choice
    case $choice in
        1)
            full_installation
            read -p "Press Enter to return to menu..."
            ;;
        2)
            update_existing
            read -p "Press Enter to return to menu..."
            ;;
        3)
            setup_shell_alias "false"
            read -p "Press Enter to return to menu..."
            ;;
        4)
            if [ -f "$BINARY_DEST" ]; then
                run_setup "false"
            else
                echo "❌ Error: Binary not found at $BINARY_DEST. Please install first."
            fi
            read -p "Press Enter to return to menu..."
            ;;
        5)
            echo "Goodbye!"
            exit 0
            ;;
        *)
            echo "❌ Invalid option."
            sleep 1
            ;;
    esac
    clear
done
