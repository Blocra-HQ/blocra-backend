#!/bin/bash
# Render deployment start script

# Set DATABASE_URL to use /tmp if not already set
export DATABASE_URL="${DATABASE_URL:-sqlite:/tmp/blocra.db}"

# Set default JWT_SECRET if not set (for testing only - set in Render dashboard for production)
export JWT_SECRET="${JWT_SECRET:-default_jwt_secret_change_in_production_12345678901234567890}"

# Set logging level
export RUST_LOG="${RUST_LOG:-info}"

echo "Starting BlocRA Backend..."
echo "DATABASE_URL: $DATABASE_URL"
echo "PORT: ${PORT:-5000}"

# Run the application
cargo run --release
