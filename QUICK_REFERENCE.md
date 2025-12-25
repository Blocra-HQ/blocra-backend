# Quick Reference - Rust Backend

## 🚀 Quick Start

```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Setup
cd backend-rust
cp .env.example .env
# Edit .env with your JWT_SECRET

# 3. Run
cargo run

# 4. Test
curl http://localhost:5000/health
```

## 📋 All API Endpoints

### Health
```
GET  /health                          # Health check
GET  /api/                            # Welcome message
```

### Authentication
```
POST /api/auth/register               # Register new user
POST /api/auth/login                  # Login with email/password
POST /api/auth/google                 # Google OAuth login
POST /api/auth/wallet                 # Wallet authentication
POST /api/auth/refresh                # Refresh JWT token
GET  /api/auth/me                     # Get current user
GET  /api/auth/profile                # Get user profile
PUT  /api/auth/profile                # Update profile
POST /api/auth/logout                 # Logout
GET  /api/auth/oauth/config           # Get OAuth config
```

### Bounties
```
GET    /api/bounties                  # List all bounties
POST   /api/bounties                  # Create bounty (auth required)
GET    /api/bounties/:id              # Get bounty by ID
PUT    /api/bounties/:id              # Update bounty (auth required)
DELETE /api/bounties/:id              # Delete bounty (auth required)
POST   /api/bounties/:id/join         # Join bounty (auth required)
POST   /api/bounties/:id/submit       # Submit work (auth required)
GET    /api/bounties/:id/participants # Get participants
GET    /api/bounties/:id/submissions  # Get submissions
```

### Contracts (RPC)
```
POST /api/contracts/query             # Query contract
POST /api/contracts/events            # Fetch contract events
POST /api/contracts/analyze           # Analyze contract
POST /api/contracts/save-query        # Save query (auth required)
GET  /api/contracts/saved-queries     # Get saved queries (auth required)
GET  /api/contracts/queries           # List queries (auth required)
GET  /api/contracts/queries/:id       # Get query by ID
```

### Dashboards
```
GET    /api/dashboards                # Get dashboard data (auth required)
GET    /api/dashboards/analytics      # Get analytics
```

### Queries
```
GET    /api/queries                   # List saved queries (auth required)
POST   /api/queries                   # Save query (auth required)
GET    /api/queries/:id               # Get query by ID
DELETE /api/queries/:id               # Delete query (auth required)
```

### Admin
```
GET    /api/admin/users               # List users (admin only)
DELETE /api/admin/users/:id           # Delete user (admin only)
POST   /api/admin/bounties/:id/approve # Approve bounty (admin only)
POST   /api/admin/submissions/:id/review # Review submission (admin only)
GET    /api/admin/stats               # Get statistics (admin only)
```

## 🔑 Authentication

### Register
```bash
curl -X POST http://localhost:5000/api/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"email":"user@example.com","username":"user123"}'
```

### Login with Wallet
```bash
curl -X POST http://localhost:5000/api/auth/wallet \
  -H 'Content-Type: application/json' \
  -d '{"wallet_address":"0x123..."}'
```

### Use Token
```bash
curl http://localhost:5000/api/auth/me \
  -H 'Authorization: Bearer YOUR_JWT_TOKEN'
```

## 🔌 RPC Examples

### Fetch Contract Events
```bash
curl -X POST http://localhost:5000/api/contracts/events \
  -H 'Content-Type: application/json' \
  -d '{
    "contractAddress": "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7",
    "fromDate": "2024-01-01T00:00:00Z",
    "toDate": "2024-12-24T00:00:00Z"
  }'
```

### Analyze Contract
```bash
curl -X POST http://localhost:5000/api/contracts/analyze \
  -H 'Content-Type: application/json' \
  -d '{
    "contractAddress": "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7"
  }'
```

## 🎯 Bounty Operations

### Create Bounty
```bash
curl -X POST http://localhost:5000/api/bounties \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer YOUR_TOKEN' \
  -d '{
    "title": "Build a feature",
    "description": "Details here",
    "reward_amount": 100.0,
    "reward_token": "STRK",
    "difficulty": "medium",
    "category": "development"
  }'
```

### Join Bounty
```bash
curl -X POST http://localhost:5000/api/bounties/BOUNTY_ID/join \
  -H 'Authorization: Bearer YOUR_TOKEN'
```

### Submit Work
```bash
curl -X POST http://localhost:5000/api/bounties/BOUNTY_ID/submit \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer YOUR_TOKEN' \
  -d '{"content": "My submission details"}'
```

## 🗄️ Database

### Location
```
./data/blocra.db
```

### Backup
```bash
cp data/blocra.db backup/blocra-$(date +%Y%m%d).db
```

### Inspect
```bash
sqlite3 data/blocra.db
.tables
SELECT * FROM users;
.quit
```

## 🔧 Configuration

### Environment Variables (.env)
```bash
# Server
PORT=5000
HOST=0.0.0.0
RUST_LOG=info

# Database
DATABASE_URL=sqlite:./data/blocra.db

# JWT
JWT_SECRET=your-secret-key-here
JWT_EXPIRATION=86400

# CORS
CORS_ORIGIN=http://localhost:5173

# Google OAuth (optional)
GOOGLE_CLIENT_ID=your-client-id
GOOGLE_CLIENT_SECRET=your-client-secret

# Rate Limiting
RATE_LIMIT_MAX_REQUESTS=100
RATE_LIMIT_WINDOW_SECS=900
```

## 🐛 Debugging

### Enable Debug Logs
```bash
RUST_LOG=debug cargo run
```

### Check Health
```bash
curl http://localhost:5000/health
```

### View Database
```bash
sqlite3 data/blocra.db "SELECT * FROM users;"
```

## 📦 Deployment

### Build Release
```bash
cargo build --release
```

### Run Binary
```bash
./target/release/blocra-backend
```

### Docker
```bash
docker build -t blocra-backend .
docker run -p 5000:5000 --env-file .env blocra-backend
```

### Systemd
```bash
sudo cp blocra-backend.service /etc/systemd/system/
sudo systemctl enable blocra-backend
sudo systemctl start blocra-backend
sudo systemctl status blocra-backend
```

## 🧪 Testing

### Run Test Script
```bash
./test-api.sh
```

### Manual Tests
```bash
# Health
curl http://localhost:5000/health

# List bounties
curl http://localhost:5000/api/bounties

# Auth (should return 401)
curl http://localhost:5000/api/auth/me
```

## 📊 Performance

### Expected Metrics
- Startup: ~50ms
- Memory: ~5MB
- Response time: ~5ms
- Requests/sec: ~15,000

### Monitor
```bash
# CPU/Memory
top -p $(pgrep blocra-backend)

# Requests
tail -f logs/app.log
```

## 🆘 Troubleshooting

### Port in use
```bash
lsof -i :5000
kill -9 <PID>
```

### Database locked
```bash
pkill blocra-backend
rm data/blocra.db-shm data/blocra.db-wal
cargo run
```

### Build errors
```bash
cargo clean
cargo build
```

## 📚 Documentation

- `README.md` - Setup guide
- `FEATURE_PARITY.md` - Complete feature list
- `RPC_INTEGRATION_COMPLETE.md` - RPC details
- `ARCHITECTURE.md` - System design
- `TROUBLESHOOTING.md` - Common issues
- `MIGRATION_GUIDE.md` - Migration steps

## 🎯 Key Files

```
backend-rust/
├── src/
│   ├── main.rs              # Entry point
│   ├── config.rs            # Configuration
│   ├── db.rs                # Database setup
│   ├── routes/              # API routes
│   ├── handlers/            # Business logic
│   ├── models/              # Data models
│   ├── services/rpc.rs      # RPC integration
│   ├── middleware/          # Middleware
│   └── utils/               # Utilities
├── migrations/              # Database migrations
├── Cargo.toml               # Dependencies
├── .env                     # Configuration
└── data/                    # SQLite database
```

## 💡 Tips

1. **Always use .env file** - Don't hardcode secrets
2. **Check logs first** - Most issues are logged
3. **Backup database** - Copy data/blocra.db regularly
4. **Use debug mode** - RUST_LOG=debug for troubleshooting
5. **Test endpoints** - Use test-api.sh script
6. **Monitor health** - Check /health endpoint
7. **Read docs** - Check TROUBLESHOOTING.md for issues

## 🚀 Next Steps

1. Test the backend: `cargo run`
2. Run API tests: `./test-api.sh`
3. Update frontend API URL (if needed)
4. Deploy to production
5. Monitor and optimize

---

**Need help?** Check the documentation files or enable debug logging with `RUST_LOG=debug cargo run`
