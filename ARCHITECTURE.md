# Rust Backend Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend (React)                      │
│                    Starknet RPC Integration                  │
└────────────────────────┬────────────────────────────────────┘
                         │ HTTP/WebSocket
                         │
┌────────────────────────▼────────────────────────────────────┐
│                    Actix-Web Server                          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              Middleware Layer                        │   │
│  │  • CORS         • Rate Limiting                      │   │
│  │  • Logging      • Security Headers                   │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │                                    │
│  ┌──────────────────────▼───────────────────────────────┐   │
│  │              Route Handlers                          │   │
│  │  /api/auth      /api/bounties    /api/admin         │   │
│  │  /api/contracts /api/dashboards  /api/queries       │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │                                    │
│  ┌──────────────────────▼───────────────────────────────┐   │
│  │              Business Logic (Handlers)               │   │
│  │  • Authentication    • Bounty Management             │   │
│  │  • User Management   • Contract Queries              │   │
│  │  • Admin Operations  • Analytics                     │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │                                    │
│  ┌──────────────────────▼───────────────────────────────┐   │
│  │              Data Access Layer (sqlx)                │   │
│  │  • Compile-time SQL validation                       │   │
│  │  • Connection pooling                                │   │
│  │  • Async queries                                     │   │
│  └──────────────────────┬───────────────────────────────┘   │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                    SQLite Database                           │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Tables:                                             │   │
│  │  • users                • bounty_participants        │   │
│  │  • bounties             • submissions                │   │
│  │  • rewards              • contract_queries           │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Request Flow

```
1. Client Request
   │
   ├─→ CORS Check
   │
   ├─→ Rate Limiting
   │
   ├─→ Request Logging
   │
   ├─→ Route Matching
   │
   ├─→ JWT Validation (if protected)
   │
   ├─→ Handler Execution
   │   │
   │   ├─→ Input Validation
   │   │
   │   ├─→ Business Logic
   │   │
   │   └─→ Database Query
   │
   └─→ Response (JSON)
```

## Module Structure

```
src/
│
├── main.rs                    # Application bootstrap
│   • Initialize logger
│   • Load configuration
│   • Setup database pool
│   • Start HTTP server
│
├── config.rs                  # Configuration management
│   • Environment variables
│   • Default values
│   • Validation
│
├── db.rs                      # Database setup
│   • Connection pool
│   • Migration runner
│   • Health checks
│
├── errors.rs                  # Error handling
│   • Custom error types
│   • HTTP error responses
│   • Error conversions
│
├── models/                    # Data models
│   ├── user.rs               # User entity
│   ├── bounty.rs             # Bounty entity
│   ├── submission.rs         # Submission entity
│   └── ...                   # Other entities
│
├── routes/                    # HTTP routes
│   ├── auth.rs               # Auth endpoints
│   ├── bounty.rs             # Bounty endpoints
│   ├── admin.rs              # Admin endpoints
│   └── ...                   # Other routes
│
├── handlers/                  # Business logic
│   ├── auth.rs               # Auth logic
│   ├── bounty.rs             # Bounty logic
│   ├── admin.rs              # Admin logic
│   └── ...                   # Other handlers
│
├── middleware/                # Middleware
│   └── rate_limit.rs         # Rate limiting
│
└── utils/                     # Utilities
    └── jwt.rs                # JWT operations
```

## Data Flow

### Authentication Flow
```
1. Client sends credentials
   ↓
2. Handler validates credentials
   ↓
3. Query database for user
   ↓
4. Generate JWT token
   ↓
5. Return token to client
   ↓
6. Client includes token in subsequent requests
   ↓
7. Middleware validates token
   ↓
8. Extract user ID from token
   ↓
9. Pass to handler
```

### Bounty Creation Flow
```
1. Client sends bounty data + JWT
   ↓
2. Middleware validates JWT
   ↓
3. Extract user ID
   ↓
4. Handler validates bounty data
   ↓
5. Generate bounty ID (UUID)
   ↓
6. Insert into database
   ↓
7. Return created bounty
```

## Database Schema

```sql
users
├── id (TEXT, PRIMARY KEY)
├── wallet_address (TEXT, UNIQUE)
├── email (TEXT, UNIQUE)
├── username (TEXT)
├── role (TEXT)
├── google_id (TEXT, UNIQUE)
├── profile_picture (TEXT)
├── created_at (TIMESTAMP)
└── updated_at (TIMESTAMP)

bounties
├── id (TEXT, PRIMARY KEY)
├── title (TEXT)
├── description (TEXT)
├── reward_amount (REAL)
├── reward_token (TEXT)
├── status (TEXT)
├── difficulty (TEXT)
├── category (TEXT)
├── created_by (TEXT, FK → users.id)
├── deadline (TIMESTAMP)
├── max_participants (INTEGER)
├── requirements (TEXT)
├── created_at (TIMESTAMP)
└── updated_at (TIMESTAMP)

bounty_participants
├── id (TEXT, PRIMARY KEY)
├── bounty_id (TEXT, FK → bounties.id)
├── user_id (TEXT, FK → users.id)
├── status (TEXT)
└── joined_at (TIMESTAMP)

submissions
├── id (TEXT, PRIMARY KEY)
├── bounty_id (TEXT, FK → bounties.id)
├── user_id (TEXT, FK → users.id)
├── content (TEXT)
├── status (TEXT)
├── feedback (TEXT)
├── submitted_at (TIMESTAMP)
└── reviewed_at (TIMESTAMP)

rewards
├── id (TEXT, PRIMARY KEY)
├── bounty_id (TEXT, FK → bounties.id)
├── user_id (TEXT, FK → users.id)
├── amount (REAL)
├── token (TEXT)
├── transaction_hash (TEXT)
├── status (TEXT)
└── created_at (TIMESTAMP)

contract_queries
├── id (TEXT, PRIMARY KEY)
├── user_id (TEXT, FK → users.id)
├── contract_address (TEXT)
├── query_type (TEXT)
├── parameters (TEXT)
├── result (TEXT)
├── status (TEXT)
└── created_at (TIMESTAMP)
```

## Security Layers

```
┌─────────────────────────────────────┐
│   1. Network Layer (HTTPS)          │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│   2. CORS Protection                │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│   3. Rate Limiting                  │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│   4. JWT Authentication             │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│   5. Input Validation               │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│   6. SQL Injection Prevention       │
│      (Compile-time checked)         │
└─────────────────────────────────────┘
```

## Performance Optimizations

### 1. Connection Pooling
- Reuse database connections
- Configurable pool size
- Automatic connection management

### 2. Async/Await
- Non-blocking I/O
- Efficient resource usage
- High concurrency

### 3. Zero-Copy Operations
- Minimize memory allocations
- Direct buffer operations
- Efficient serialization

### 4. Compile-Time Optimizations
- SQL queries validated at compile time
- Type checking eliminates runtime errors
- Dead code elimination

### 5. SQLite Optimizations
- WAL mode for better concurrency
- Memory-mapped I/O
- Query optimization

## Deployment Architecture

### Development
```
Developer Machine
├── cargo run
└── SQLite file (./data/blocra.db)
```

### Production (Single Server)
```
Server
├── Rust Binary (systemd service)
├── SQLite file (persistent volume)
└── Nginx (reverse proxy)
```

### Production (Docker)
```
Container
├── Rust Binary
├── SQLite file (mounted volume)
└── Port 5000 exposed
```

### Production (Kubernetes)
```
Pod
├── Rust Container
├── SQLite PVC (Persistent Volume Claim)
└── Service (LoadBalancer)
```

## Monitoring & Observability

```
Application Logs
├── RUST_LOG=info  → Info level
├── RUST_LOG=debug → Debug level
└── RUST_LOG=trace → Trace level

Health Endpoint
└── GET /health
    ├── uptime
    ├── status
    └── timestamp

Metrics (Future)
├── Request count
├── Response times
├── Error rates
└── Database stats
```

## Scalability Considerations

### Vertical Scaling
- Increase CPU/RAM
- Adjust connection pool size
- Optimize query performance

### Horizontal Scaling (Future)
- Multiple instances behind load balancer
- Shared SQLite file (NFS/EFS)
- Or migrate to PostgreSQL for true multi-writer

### Caching (Future)
- Redis for session storage
- In-memory cache for hot data
- Query result caching

## Comparison with Node.js Backend

| Aspect | Node.js | Rust |
|--------|---------|------|
| Runtime | V8 Engine | Native Binary |
| Memory | ~150MB | ~5MB |
| Startup | ~2s | ~50ms |
| Concurrency | Event Loop | Tokio Runtime |
| Type Safety | Runtime | Compile-time |
| Database | MongoDB | SQLite |
| Deployment | node_modules | Single Binary |
| Performance | Good | Excellent |

## Future Enhancements

1. **WebSocket Support**
   - Real-time updates
   - Live bounty notifications
   - Chat functionality

2. **Caching Layer**
   - Redis integration
   - Query result caching
   - Session management

3. **Metrics & Monitoring**
   - Prometheus metrics
   - Grafana dashboards
   - Alert system

4. **Advanced Features**
   - GraphQL API
   - gRPC endpoints
   - Message queue integration

5. **Database Options**
   - PostgreSQL support
   - MySQL support
   - Database replication
