#!/bin/bash

# Exit on error
set -e

# Configuration Setup
CONFIG_DIR="$HOME/.config/mylm"
CONFIG_FILE="$CONFIG_DIR/mylm.yaml"

build_binary() {
    echo "🚀 Building mylm in release mode to ensure the latest version..."
    cargo build --release
}

install_binary() {
    echo "📦 Installing/Updating binary to /usr/local/bin/ai..."
    # The binary name is 'mylm' as per Cargo.toml, but we want it available as 'ai'
    sudo cp target/release/mylm /usr/local/bin/ai
    sudo chmod +x /usr/local/bin/ai
    echo "✅ Binary installed successfully to /usr/local/bin/ai."
}

setup_shell_alias() {
    echo ""
    echo "🔍 Checking for shell alias conflicts..."
    
    local shell_rc=""
    if [[ "$SHELL" == *"zsh"* ]]; then
        shell_rc="$HOME/.zshrc"
    elif [[ "$SHELL" == *"bash"* ]]; then
        shell_rc="$HOME/.bashrc"
    fi

    if [ -n "$shell_rc" ]; then
        if grep -q "alias ai=" "$shell_rc"; then
            echo "⚠️  Found an existing 'ai' alias in $shell_rc."
            read -p "Would you like to replace it with 'alias ai=/usr/local/bin/ai'? [y/N]: " replace_alias
            if [[ "$replace_alias" =~ ^[Yy]$ ]]; then
                # Remove existing ai alias lines
                sed -i '/alias ai=/d' "$shell_rc"
                echo "alias ai='/usr/local/bin/ai'" >> "$shell_rc"
                echo "✅ Alias updated in $shell_rc. Please restart your shell or run 'source $shell_rc'."
            fi
        else
            read -p "Would you like to add 'alias ai=/usr/local/bin/ai' to your $shell_rc for better compatibility? [y/N]: " add_alias
            if [[ "$add_alias" =~ ^[Yy]$ ]]; then
                echo "alias ai='/usr/local/bin/ai'" >> "$shell_rc"
                echo "✅ Alias added to $shell_rc. Please restart your shell or run 'source $shell_rc'."
            fi
        fi
    else
        echo "⚠️  Could not determine your shell configuration file (only bash and zsh are supported for auto-aliasing)."
    fi
}

run_setup() {
    echo ""
    echo "⚙️  Configuring mylm..."
    # Call the setup wizard built into the binary
    # This will also trigger the model warmup and web search setup
    if [ -f "/usr/local/bin/ai" ]; then
        /usr/local/bin/ai setup
    else
        ./target/release/mylm setup
    fi
}

show_menu() {
    echo "------------------------------------------"
    echo "   🤖 mylm Installation & Setup Wizard   "
    echo "------------------------------------------"
    echo "1) 🚀 Full Installation (Build + Install + Alias + Setup)"
    echo "2) 📦 Build & Install Binary Only"
    echo "3) 🔗 Setup Shell Alias (ai default)"
    echo "4) ⚙️  Run Configuration Setup (ai setup)"
    echo "5) ❌ Exit"
    echo "------------------------------------------"
}

while true; do
    show_menu
    read -p "Select an option [1-5]: " choice
    case $choice in
        1)
            build_binary
            install_binary
            setup_shell_alias
            run_setup
            echo ""
            echo "✅ Full installation complete! You can now use 'ai' from your terminal."
            read -p "Press Enter to return to menu..."
            ;;
        2)
            build_binary
            install_binary
            echo ""
            echo "✅ Binary installed! Run 'ai setup' if this is your first time."
            read -p "Press Enter to return to menu..."
            ;;
        3)
            setup_shell_alias
            read -p "Press Enter to return to menu..."
            ;;
        4)
            # Check if binary exists
            if [ ! -f "/usr/local/bin/ai" ] && [ ! -f "./target/release/mylm" ]; then
                echo "❌ Error: Binary not found. Please build first (Option 1 or 2)."
            else
                run_setup
            fi
            read -p "Press Enter to return to menu..."
            ;;
        5)
            echo "Goodbye!"
            exit 0
            ;;
        *)
            echo "❌ Invalid option. Please try again."
            sleep 1
            ;;
    esac
    clear
done
