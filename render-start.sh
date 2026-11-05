#!/bin/bash
# Render deployment start script

# Set DATABASE_URL to use /tmp if not already set
export DATABASE_URL="${DATABASE_URL:-sqlite:/tmp/blocra.db}"

# JWT_SECRET has no default: set it in the Render dashboard (openssl rand -base64 32).
if [ -z "${JWT_SECRET:-}" ]; then
  echo "JWT_SECRET is not set; refusing to start." >&2
  exit 1
fi

# Set logging level
export RUST_LOG="${RUST_LOG:-info}"

echo "Starting BlocRA Backend..."
echo "DATABASE_URL: $DATABASE_URL"
echo "PORT: ${PORT:-5000}"

# Run the application
cargo run --release
