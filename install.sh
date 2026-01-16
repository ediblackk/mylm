#!/bin/bash

# Exit on error
set -e

# Configuration Setup
CONFIG_DIR="$HOME/.config/mylm"
CONFIG_FILE="$CONFIG_DIR/mylm.yaml"
# NOTE: This installer is intentionally "no-sudo".
# Install into user-space by default.
PREFIX="${MYLM_PREFIX:-$HOME/.local}"
BINARY_DEST="$PREFIX/bin/mylm"
BUILD_PROFILE="release"

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

check_busy() {
    local target="$1"
    if [ -f "$target" ]; then
        if command -v fuser &> /dev/null; then
            if fuser "$target" >/dev/null 2>&1; then
                echo "⚠️  Binary $target is currently in use (Text file busy)."
                read -p "Kill running processes using it? [y/N]: " kill_it
                if [[ "$kill_it" =~ ^[Yy]$ ]]; then
                    # Try non-sudo first if we own the file, else sudo
                    if [ -w "$target" ]; then
                        fuser -k -TERM "$target" >/dev/null 2>&1 || true
                    else
                        sudo fuser -k -TERM "$target" >/dev/null 2>&1 || true
                    fi
                    sleep 0.5
                    if fuser "$target" >/dev/null 2>&1; then
                        if [ -w "$target" ]; then
                            fuser -k -KILL "$target" >/dev/null 2>&1 || true
                        else
                            sudo fuser -k -KILL "$target" >/dev/null 2>&1 || true
                        fi
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

check_and_install_dependencies() {
    echo "🔍 Checking for system dependencies..."

    # Check for Rust/Cargo FIRST
    if ! command -v cargo &> /dev/null; then
        echo "❌ Rust/Cargo not found."
        read -p "Would you like to install Rust now via rustup.rs? [Y/n]: " install_rust
        if [[ ! "$install_rust" =~ ^[Nn]$ ]]; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source "$HOME/.cargo/env"
            echo "✅ Rust installed."
            echo "⚠️  IMPORTANT: You MUST restart your terminal or run 'source \$HOME/.cargo/env' before continuing with other steps if you are running this in a new shell."
            echo "💡 This script will continue now using the sourced environment."
        else
            echo "❌ Error: Rust is required to build mylm. Exiting."
            exit 1
        fi
    fi

    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS=$ID
    else
        OS=$(uname -s)
    fi

    MISSING_DEPS=()

    # This script will NEVER attempt to install system packages (no sudo).
    # We only detect + print actionable guidance.
    detect_pm() {
        if command -v apt-get &> /dev/null; then echo "apt"; return 0; fi
        if command -v dnf &> /dev/null; then echo "dnf"; return 0; fi
        if command -v pacman &> /dev/null; then echo "pacman"; return 0; fi
        if command -v zypper &> /dev/null; then echo "zypper"; return 0; fi
        if command -v apk &> /dev/null; then echo "apk"; return 0; fi
        if command -v xbps-install &> /dev/null; then echo "xbps"; return 0; fi
        if command -v emerge &> /dev/null; then echo "emerge"; return 0; fi
        if command -v nix-env &> /dev/null || command -v nix &> /dev/null; then echo "nix"; return 0; fi
        if command -v brew &> /dev/null; then echo "brew"; return 0; fi
        echo "unknown"
    }

    print_install_guidance() {
        local pm
        pm="$(detect_pm)"

        echo ""
        echo "⚠️  Missing system dependencies: ${MISSING_DEPS[*]}"
        echo "🔒 This installer runs without sudo, so it will NOT install system packages for you."
        echo "➡️  Install the missing packages using your system's package manager, then re-run this script."
        echo ""

        case "$pm" in
            apt)
                echo "Debian/Ubuntu example:"
                echo "  sudo apt-get update && sudo apt-get install -y ${MISSING_DEPS[*]}"
                ;;
            dnf)
                echo "Fedora example:"
                echo "  sudo dnf install -y ${MISSING_DEPS[*]}"
                ;;
            pacman)
                echo "Arch example:"
                echo "  sudo pacman -S --needed ${MISSING_DEPS[*]}"
                ;;
            zypper)
                echo "OpenSUSE example (package names may differ):"
                echo "  sudo zypper install ${MISSING_DEPS[*]}"
                ;;
            apk)
                echo "Alpine example (package names differ; may need -dev variants):"
                echo "  doas apk add ${MISSING_DEPS[*]}"
                ;;
            xbps)
                echo "Void Linux example (package names may differ):"
                echo "  sudo xbps-install -S ${MISSING_DEPS[*]}"
                ;;
            emerge)
                echo "Gentoo example (use emerge equivalents):"
                echo "  sudo emerge ${MISSING_DEPS[*]}"
                ;;
            nix)
                echo "Nix example (prefer a nix shell/flake; names differ):"
                echo "  nix shell nixpkgs#openssl nixpkgs#pkg-config nixpkgs#clang nixpkgs#cmake nixpkgs#tmux nixpkgs#protobuf"
                ;;
            brew)
                echo "Homebrew example (macOS/Linuxbrew; names may differ):"
                echo "  brew install ${MISSING_DEPS[*]}"
                ;;
            *)
                echo "Package manager not detected. Install the packages manually for your distro."
                ;;
        esac
        echo ""
        echo "🛑 Aborting build until dependencies are installed."
        exit 1
    }
    
    case $OS in
        ubuntu|debian|pop|mint)
            # Core build tools and libraries for mylm (OpenSSL, XCB, etc.) + tmux for terminal context
            DEPS=("pkg-config" "libssl-dev" "libxcb1-dev" "libxcb-render0-dev" "libxcb-shape0-dev" "libxcb-xfixes0-dev" "clang" "build-essential" "cmake" "tmux" "mold" "protobuf-compiler")
            for dep in "${DEPS[@]}"; do
                if ! dpkg -l | grep -qw "$dep" &>/dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                print_install_guidance
            fi
            ;;
        fedora)
            DEPS=("pkgconf-pkg-config" "openssl-devel" "libxcb-devel" "clang" "gcc-c++" "cmake" "tmux" "mold" "protobuf-compiler")
            for dep in "${DEPS[@]}"; do
                if ! rpm -q "$dep" &> /dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                print_install_guidance
            fi
            ;;
        arch)
            DEPS=("pkgconf" "openssl" "libxcb" "clang" "base-devel" "cmake" "tmux" "mold" "protobuf")
            for dep in "${DEPS[@]}"; do
                if ! pacman -Qs "$dep" &> /dev/null; then
                    MISSING_DEPS+=("$dep")
                fi
            done
            if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
                print_install_guidance
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
                    print_install_guidance
                fi
            else
                echo "⚠️  Homebrew not found. Please ensure you have 'openssl', 'pkg-config', and 'tmux' installed manually."
            fi
            ;;
        *)
            echo "⚠️  Unsupported or unknown OS: $OS."
            echo "🔒 This installer runs without sudo, so it cannot install system dependencies."
            echo "➡️  Ensure build dependencies are installed (OpenSSL dev libs, libxcb dev libs, clang, cmake, protobuf, tmux)."
            ;;
    esac

    # sccache for faster builds (required by .cargo/config.toml)
    if ! command -v sccache &> /dev/null; then
        echo "⚠️  sccache is not installed, but it is required by .cargo/config.toml for this project."
        read -p "Would you like to install sccache now? [Y/n]: " install_sccache
        if [[ ! "$install_sccache" =~ ^[Nn]$ ]]; then
            echo "🚀 Installing sccache..."
            cargo install sccache || echo "⚠️  Failed to install sccache via cargo."
        else
            echo "⚠️  Warning: Build will likely fail if sccache is missing."
        fi
    fi
}

build_binary() {
    local force_rebuild=$1
    local initial_profile=$2
    
    # 1. Smart detection of existing binary profile
    if [ -z "$initial_profile" ]; then
        if [ -f "target/release/mylm" ] && [ -f "target/debug/mylm" ]; then
            if [ "target/release/mylm" -nt "target/debug/mylm" ]; then
                initial_profile="release"
            else
                initial_profile="debug"
            fi
        elif [ -f "target/release/mylm" ]; then
            initial_profile="release"
        elif [ -f "target/debug/mylm" ]; then
            initial_profile="debug"
        fi
    fi

    # 2. If still no profile and it's a fresh install or forced, ask the user
    if [ -z "$initial_profile" ]; then
        read -p "Use optimized release build (20 min) or fast dev build (7 min)? [r/D]: " build_type
        if [[ "$build_type" =~ ^[Rr]$ ]]; then
            BUILD_PROFILE="release"
        else
            BUILD_PROFILE="debug"
        fi
    else
        BUILD_PROFILE="$initial_profile"
    fi

    if [ "$force_rebuild" != "true" ] && [ -f "./target/$BUILD_PROFILE/mylm" ]; then
        echo "✨ Found an existing $BUILD_PROFILE binary at ./target/$BUILD_PROFILE/mylm."
        read -p "Would you like to rebuild it to ensure it's the latest version? [y/N]: " rebuild
        if [[ ! "$rebuild" =~ ^[Yy]$ ]]; then
            echo "⏭️  Skipping build, using existing binary."
            return 0
        fi
    fi

    echo "🚀 Building mylm in $BUILD_PROFILE mode..."
    
    if [ "$BUILD_PROFILE" == "release" ]; then
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
    local profile=$1
    
    # Final safety detection for the cp command
    if [ -z "$profile" ] || [ ! -f "target/$profile/mylm" ]; then
        if [ -f "target/release/mylm" ] && [ -f "target/debug/mylm" ]; then
            if [ "target/release/mylm" -nt "target/debug/mylm" ]; then
                profile="release"
            else
                profile="debug"
            fi
        elif [ -f "target/release/mylm" ]; then
            profile="release"
        elif [ -f "target/debug/mylm" ]; then
            profile="debug"
        fi
    fi

    if [ -z "$profile" ] || [ ! -f "target/$profile/mylm" ]; then
        echo "❌ Error: Could not find binary in target/release or target/debug."
        echo "   Please ensure the build completed successfully."
        exit 1
    fi

    echo "📦 Installing binary from target/$profile/mylm to $BINARY_DEST..."
    mkdir -p "$(dirname "$BINARY_DEST")"
    
    check_busy "$BINARY_DEST"
    
    cp "target/$profile/mylm" "$BINARY_DEST"
    chmod +x "$BINARY_DEST"
    
    echo "✅ Binary installed successfully."
}

ensure_path_has_prefix_bin() {
    local bin_dir
    bin_dir="$(dirname "$BINARY_DEST")"

    # Already in PATH
    if echo ":$PATH:" | grep -q ":${bin_dir}:"; then
        return 0
    fi

    echo ""
    echo "🔍 Ensuring $bin_dir is on your PATH..."

    # Determine shell config
    local shell_rc=""
    if [[ "${SHELL:-}" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
        if ! grep -q "${bin_dir}" "$shell_rc" 2>/dev/null; then
            echo "export PATH=\"${bin_dir}:\$PATH\"" >> "$shell_rc"
            echo "✅ Added PATH update to $shell_rc"
        fi
    elif [[ "${SHELL:-}" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
        if ! grep -q "${bin_dir}" "$shell_rc" 2>/dev/null; then
            echo "export PATH=\"${bin_dir}:\$PATH\"" >> "$shell_rc"
            echo "✅ Added PATH update to $shell_rc"
        fi
    elif [[ "${SHELL:-}" == *"fish"* ]]; then
        shell_rc="$HOME/.config/fish/config.fish"
        mkdir -p "$(dirname "$shell_rc")"
        if ! grep -q "${bin_dir}" "$shell_rc" 2>/dev/null; then
            echo "set -gx PATH ${bin_dir} \$PATH" >> "$shell_rc"
            echo "✅ Added PATH update to $shell_rc"
        fi
    else
        echo "⚠️  Could not detect your shell. Ensure '$bin_dir' is on your PATH manually."
        return 0
    fi

    echo "💡 Restart your shell or re-source your shell config for PATH changes to take effect."
}

setup_shell_alias() {
    local mandatory=$1
    echo ""
    echo "🔍 Configuring shell alias..."
    
    local shell_rc=""
    if [[ "${SHELL:-}" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "${SHELL:-}" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    elif [[ "${SHELL:-}" == *"fish"* ]]; then
        shell_rc="$HOME/.config/fish/config.fish"
        mkdir -p "$(dirname "$shell_rc")"
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

        if grep -q "alias $chosen_alias=" "$shell_rc" 2>/dev/null; then
            echo "⚠️  Found an existing '$chosen_alias' alias in $shell_rc."
            if [ "$mandatory" == "true" ]; then
                if command -v perl &> /dev/null; then
                    perl -0777 -i -pe "s/^alias\s+${chosen_alias}=.*\n//mg" "$shell_rc"
                else
                    sed -i "/alias $chosen_alias=/d" "$shell_rc" 2>/dev/null || true
                fi
                if [[ "$shell_rc" == *"config.fish" ]]; then
                    echo "alias $chosen_alias '$BINARY_DEST'" >> "$shell_rc"
                else
                    echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
                fi
                echo "✅ Alias updated in $shell_rc."
            else
                read -p "Would you like to replace it? [y/N]: " replace_alias
                if [[ "$replace_alias" =~ ^[Yy]$ ]]; then
                    if command -v perl &> /dev/null; then
                        perl -0777 -i -pe "s/^alias\s+${chosen_alias}=.*\n//mg" "$shell_rc"
                    else
                        sed -i "/alias $chosen_alias=/d" "$shell_rc" 2>/dev/null || true
                    fi
                    if [[ "$shell_rc" == *"config.fish" ]]; then
                        echo "alias $chosen_alias '$BINARY_DEST'" >> "$shell_rc"
                    else
                        echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
                    fi
                    echo "✅ Alias updated in $shell_rc."
                fi
            fi
        else
            if [[ "$shell_rc" == *"config.fish" ]]; then
                echo "alias $chosen_alias '$BINARY_DEST'" >> "$shell_rc"
            else
                echo "alias $chosen_alias='$BINARY_DEST'" >> "$shell_rc"
            fi
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
    if [[ "${SHELL:-}" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "${SHELL:-}" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    elif [[ "${SHELL:-}" == *"fish"* ]]; then
        shell_rc="$HOME/.config/fish/config.fish"
        mkdir -p "$(dirname "$shell_rc")"
    fi

    local snippet_start="# --- mylm tmux auto-start ---"
    local snippet_end="# --- end mylm tmux auto-start ---"

    # If snippet already exists, upgrade legacy shared-session behavior (causes mirroring)
    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ] && grep -q "$snippet_start" "$shell_rc"; then
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
            if [[ "$shell_rc" == *"config.fish" ]]; then
                echo 'if type -q tmux; and test -z "$TMUX"; and status is-interactive' >> "$shell_rc"
                echo '    tmux new-session -s "mylm-"(date +%s)"-"(echo %self)"-"(random)' >> "$shell_rc"
                echo 'end' >> "$shell_rc"
            else
                echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
                echo '    tmux new-session -s "mylm-$(date +%s)-$$-$RANDOM"' >> "$shell_rc"
                echo 'fi' >> "$shell_rc"
            fi
            echo "$snippet_end" >> "$shell_rc"

            echo "✅ Upgraded tmux auto-start snippet in $shell_rc."
            echo "💡 Changes will take effect in new terminal sessions."
            return 0
        fi

        echo "✅ tmux auto-start is already configured in $shell_rc."
        return 0
    fi

    read -p "Enable global seamless context via tmux? [y/N]: " enable_tmux
    if [[ ! "$enable_tmux" =~ ^[Yy]$ ]]; then
        echo "⏭️  Skipping tmux auto-start setup."
        return 0
    fi

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        echo "" >> "$shell_rc"
        echo "$snippet_start" >> "$shell_rc"
        if [[ "$shell_rc" == *"config.fish" ]]; then
            echo 'if type -q tmux; and test -z "$TMUX"; and status is-interactive' >> "$shell_rc"
            echo '    tmux new-session -s "mylm-"(date +%s)"-"(echo %self)"-"(random)' >> "$shell_rc"
            echo 'end' >> "$shell_rc"
        else
            echo 'if command -v tmux &> /dev/null && [ -z "$TMUX" ] && [ -n "$PS1" ]; then' >> "$shell_rc"
            echo '    tmux new-session -s "mylm-$(date +%s)-$$-$RANDOM"' >> "$shell_rc"
            echo 'fi' >> "$shell_rc"
        fi
        echo "$snippet_end" >> "$shell_rc"
        echo "✅ Added tmux auto-start snippet to $shell_rc."
        echo "💡 Changes will take effect in new terminal sessions."
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
    
    build_binary "true"
    install_binary "$BUILD_PROFILE"
    ensure_path_has_prefix_bin
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

    build_binary "false"
    install_binary "$BUILD_PROFILE"
    ensure_path_has_prefix_bin
    
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
