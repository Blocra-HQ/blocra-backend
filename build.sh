#!/bin/bash
set -e

echo "🦀 Building BlocRA Rust Backend..."

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust is not installed. Install from https://rustup.rs"
    exit 1
fi

# Check if .env exists
if [ ! -f .env ]; then
    echo "⚠️  .env not found. Copying from .env.example..."
    cp .env.example .env
    echo "📝 Please edit .env with your configuration"
fi

# Build in release mode
echo "🔨 Building release binary..."
cargo build --release

echo "✅ Build complete!"
echo ""
echo "To run the server:"
echo "  ./target/release/blocra-backend"
echo ""
echo "Or for development:"
echo "  cargo run"
