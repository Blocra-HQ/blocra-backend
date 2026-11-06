#!/bin/sh
# Container start-up: make the data volume writable for the unprivileged
# blocra user (hosted disks such as Render's are mounted owned by root),
# keep uploaded profile images on that volume, then run the server as
# blocra.
set -e
DATA_DIR="${DATA_DIR:-/app/data}"
mkdir -p "$DATA_DIR/uploads"
if [ "$(id -u)" = "0" ]; then
  chown -R blocra:blocra "$DATA_DIR"
fi
# /app/uploads (served at /uploads) lives on the volume so images survive redeploys.
# If something is mounted at /app/uploads already, keep it as it is.
if [ ! -L /app/uploads ]; then
  if rm -rf /app/uploads 2>/dev/null; then
    ln -s "$DATA_DIR/uploads" /app/uploads
  else
    echo "Keeping the volume mounted at /app/uploads"
    [ "$(id -u)" = "0" ] && chown blocra:blocra /app/uploads
  fi
fi
if [ "$(id -u)" = "0" ]; then
  exec setpriv --reuid=blocra --regid=blocra --init-groups /app/blocra-backend "$@"
fi
exec /app/blocra-backend "$@"
