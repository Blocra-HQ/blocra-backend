# BlocRA Backend - Rust Edition

High-performance Rust backend with SQLite database for the BlocRA platform.

## Features

- **Fast & Efficient**: Built with Actix-web, one of the fastest web frameworks
- **SQLite Database**: Lightweight, serverless database with zero configuration
- **Type-Safe**: Compile-time SQL query validation with sqlx
- **Secure**: JWT authentication, rate limiting, CORS protection
- **API Compatible**: Drop-in replacement for the Node.js backend

## Prerequisites

- Rust 1.70+ (install from https://rustup.rs)
- SQLite 3

## Quick Start

1. **Copy environment variables**:
   ```bash
   cp .env.example .env
   ```

2. **Edit `.env` with your configuration**

3. **Build and run**:
   ```bash
   cargo run
   ```

   Or for production:
   ```bash
   cargo build --release
   ./target/release/blocra-backend
   ```

## Development

```bash
# Run with auto-reload (install cargo-watch first)
cargo install cargo-watch
cargo watch -x run

# Run tests
cargo test

# Check code
cargo clippy
```

## API Endpoints

All endpoints are compatible with the original Node.js backend:

### Authentication
- `POST /api/auth/google` - Google OAuth login
- `POST /api/auth/wallet` - Wallet authentication
- `GET /api/auth/me` - Get current user
- `POST /api/auth/logout` - Logout

### Bounties
- `GET /api/bounties` - List all bounties
- `POST /api/bounties` - Create bounty
- `GET /api/bounties/:id` - Get bounty details
- `PUT /api/bounties/:id` - Update bounty
- `DELETE /api/bounties/:id` - Delete bounty
- `POST /api/bounties/:id/join` - Join bounty
- `POST /api/bounties/:id/submit` - Submit work

### Admin
- `GET /api/admin/users` - List users
- `DELETE /api/admin/users/:id` - Delete user
- `POST /api/admin/bounties/:id/approve` - Approve bounty
- `POST /api/admin/submissions/:id/review` - Review submission
- `GET /api/admin/stats` - Get statistics

### Contracts
- `POST /api/contracts/query` - Query contract
- `GET /api/contracts/queries` - List queries

### Dashboard
- `GET /api/dashboards` - Get dashboard data
- `GET /api/dashboards/analytics` - Get analytics

## Database

SQLite database is automatically created at `./data/blocra.db`. Migrations run automatically on startup.

## Migration from Node.js

1. Stop the Node.js backend
2. Export data from MongoDB (if needed)
3. Start Rust backend
4. Update frontend API URL if needed (default: http://localhost:5000)

The Rust backend maintains the same API contract, so no frontend changes are required.

## Performance

Expected improvements over Node.js:
- 2-3x faster response times
- 50% lower memory usage
- Better concurrency handling
- Smaller binary size (~10MB vs 100MB+ node_modules)

## License

Same as parent project
