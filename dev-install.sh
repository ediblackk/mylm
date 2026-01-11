#!/bin/bash
echo "🚀 Quick dev install..."
cargo build
sudo cp target/debug/mylm /usr/local/bin/mylm
sudo chmod +x /usr/local/bin/mylm
echo "✅ Dev binary installed! (Use install.sh for optimized release)"
