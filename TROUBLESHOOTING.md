# Troubleshooting Guide

## Common Issues and Solutions

### Build Issues

#### Issue: "cargo: command not found"
**Cause**: Rust is not installed or not in PATH

**Solution**:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add to PATH
source $HOME/.cargo/env

# Verify installation
cargo --version
```

#### Issue: "failed to compile sqlx"
**Cause**: Missing system dependencies

**Solution**:
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install pkg-config libssl-dev

# macOS
brew install openssl pkg-config

# Fedora/RHEL
sudo dnf install openssl-devel pkg-config
```

#### Issue: "linking with `cc` failed"
**Cause**: Missing C compiler

**Solution**:
```bash
# Ubuntu/Debian
sudo apt-get install build-essential

# macOS
xcode-select --install

# Fedora/RHEL
sudo dnf groupinstall "Development Tools"
```

### Runtime Issues

#### Issue: "JWT_SECRET must be set"
**Cause**: Missing environment variable

**Solution**:
```bash
# Copy example env file
cp .env.example .env

# Edit .env and set JWT_SECRET
nano .env

# Or set directly
export JWT_SECRET="your-secret-key-here"
```

#### Issue: "Failed to create database pool"
**Cause**: Database file or directory doesn't exist

**Solution**:
```bash
# Create data directory
mkdir -p data

# Check permissions
chmod 755 data

# Verify DATABASE_URL in .env
cat .env | grep DATABASE_URL
```

#### Issue: "Address already in use (os error 98)"
**Cause**: Port 5000 is already in use

**Solution**:
```bash
# Find process using port 5000
lsof -i :5000
# or
netstat -tulpn | grep 5000

# Kill the process
kill -9 <PID>

# Or change port in .env
echo "PORT=5001" >> .env
```

#### Issue: "Failed to run migrations"
**Cause**: Database is locked or corrupted

**Solution**:
```bash
# Stop all instances
pkill blocra-backend

# Remove database (WARNING: deletes all data)
rm data/blocra.db*

# Restart application (will recreate database)
cargo run
```

### Database Issues

#### Issue: "database is locked"
**Cause**: Multiple processes accessing SQLite

**Solution**:
```bash
# Stop all instances
pkill blocra-backend

# Check for stale lock files
rm data/blocra.db-shm data/blocra.db-wal

# Restart with single instance
cargo run
```

#### Issue: "no such table: users"
**Cause**: Migrations haven't run

**Solution**:
```bash
# Migrations run automatically on startup
# If they fail, check logs:
RUST_LOG=debug cargo run

# Manual migration (if needed)
sqlx migrate run --database-url sqlite:./data/blocra.db
```

#### Issue: "UNIQUE constraint failed"
**Cause**: Trying to insert duplicate data

**Solution**:
- Check if user/bounty already exists
- Use proper error handling in frontend
- This is expected behavior for duplicate entries

### Authentication Issues

#### Issue: "Invalid token"
**Cause**: Token expired or JWT_SECRET changed

**Solution**:
```bash
# User needs to login again
# Ensure JWT_SECRET is consistent across restarts

# Check token expiration in .env
cat .env | grep JWT_EXPIRATION
```

#### Issue: "Missing authorization header"
**Cause**: Frontend not sending token

**Solution**:
```javascript
// Ensure frontend includes token
fetch('/api/bounties', {
  headers: {
    'Authorization': `Bearer ${token}`
  }
})
```

### CORS Issues

#### Issue: "CORS policy: No 'Access-Control-Allow-Origin' header"
**Cause**: Frontend origin not allowed

**Solution**:
```bash
# Update CORS_ORIGIN in .env
echo "CORS_ORIGIN=http://localhost:5173" >> .env

# Or allow all (development only!)
echo "CORS_ORIGIN=*" >> .env

# Restart server
```

### Performance Issues

#### Issue: "Slow query performance"
**Cause**: Missing indexes or large dataset

**Solution**:
```sql
-- Check query plan
EXPLAIN QUERY PLAN SELECT * FROM bounties WHERE status = 'open';

-- Add indexes if needed (already included in migrations)
CREATE INDEX idx_bounties_status ON bounties(status);
```

#### Issue: "High memory usage"
**Cause**: Connection pool too large

**Solution**:
```rust
// In src/db.rs, adjust max_connections
SqlitePoolOptions::new()
    .max_connections(5)  // Reduce from 10
    .connect(database_url)
```

### Deployment Issues

#### Issue: "Permission denied" when running binary
**Cause**: Binary not executable

**Solution**:
```bash
chmod +x target/release/blocra-backend
./target/release/blocra-backend
```

#### Issue: "Cannot find data directory"
**Cause**: Running from wrong directory

**Solution**:
```bash
# Always run from project root
cd /path/to/backend-rust
./target/release/blocra-backend

# Or use absolute path in .env
DATABASE_URL=sqlite:/absolute/path/to/data/blocra.db
```

#### Issue: "Systemd service fails to start"
**Cause**: Incorrect service configuration

**Solution**:
```bash
# Check service status
sudo systemctl status blocra-backend

# View logs
sudo journalctl -u blocra-backend -f

# Verify paths in service file
sudo nano /etc/systemd/system/blocra-backend.service

# Reload and restart
sudo systemctl daemon-reload
sudo systemctl restart blocra-backend
```

## Debugging Tips

### Enable Debug Logging
```bash
# Maximum verbosity
RUST_LOG=trace cargo run

# Debug level
RUST_LOG=debug cargo run

# Specific module
RUST_LOG=blocra_backend::handlers=debug cargo run
```

### Check Health Endpoint
```bash
curl http://localhost:5000/health
```

### Test Database Connection
```bash
# Install sqlite3
sudo apt-get install sqlite3

# Connect to database
sqlite3 data/blocra.db

# Check tables
.tables

# Query users
SELECT * FROM users;

# Exit
.quit
```

### Monitor Logs
```bash
# Development
cargo run 2>&1 | tee logs/app.log

# Production
tail -f /var/log/blocra-backend.log
```

### Profile Performance
```bash
# Install flamegraph
cargo install flamegraph

# Generate profile
cargo flamegraph

# View flamegraph.svg in browser
```

## Getting Help

### Check Documentation
1. README.md - Setup instructions
2. ARCHITECTURE.md - System design
3. MIGRATION_GUIDE.md - Migration steps
4. COMPARISON.md - Feature comparison

### Enable Verbose Logging
```bash
RUST_LOG=debug cargo run 2>&1 | tee debug.log
```

### Collect System Info
```bash
# Rust version
rustc --version
cargo --version

# System info
uname -a

# Environment
env | grep -E '(PORT|DATABASE|JWT|CORS)'

# Process info
ps aux | grep blocra
```

### Test API Manually
```bash
# Health check
curl -v http://localhost:5000/health

# List bounties
curl -v http://localhost:5000/api/bounties

# Test auth (should return 401)
curl -v http://localhost:5000/api/auth/me
```

## Quick Fixes

### Reset Everything
```bash
# Stop server
pkill blocra-backend

# Clean build
cargo clean

# Remove database
rm -rf data/

# Rebuild
cargo build --release

# Start fresh
./target/release/blocra-backend
```

### Fresh Database
```bash
# Backup current database
cp data/blocra.db data/blocra.db.backup

# Remove database
rm data/blocra.db*

# Restart (will recreate)
cargo run
```

### Update Dependencies
```bash
# Update Cargo.lock
cargo update

# Rebuild
cargo build --release
```

## Still Having Issues?

1. **Check the logs**: Most issues are logged with helpful error messages
2. **Search GitHub issues**: Someone may have had the same problem
3. **Ask for help**: Include logs, system info, and steps to reproduce
4. **Rollback**: You can always go back to the Node.js backend

## Useful Commands

```bash
# Check if server is running
curl http://localhost:5000/health

# View active connections
lsof -i :5000

# Monitor resource usage
top -p $(pgrep blocra-backend)

# Check database size
du -h data/blocra.db

# Backup database
cp data/blocra.db backup/blocra-$(date +%Y%m%d-%H%M%S).db

# Test with curl
curl -X POST http://localhost:5000/api/auth/wallet \
  -H 'Content-Type: application/json' \
  -d '{"wallet_address":"0x123"}'
```

## Prevention Tips

1. **Always use .env file**: Don't hardcode secrets
2. **Regular backups**: Copy database file regularly
3. **Monitor logs**: Watch for warnings
4. **Test before deploy**: Run tests in staging
5. **Keep dependencies updated**: `cargo update` regularly
6. **Use version control**: Commit working configurations
7. **Document changes**: Note any custom modifications
8. **Health checks**: Monitor `/health` endpoint
9. **Resource limits**: Set appropriate connection pool sizes
10. **Graceful shutdown**: Use proper signals (SIGTERM)
