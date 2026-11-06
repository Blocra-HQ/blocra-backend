# blocra-backend

[![Backend](https://github.com/Blocra-HQ/blocra-backend/actions/workflows/ci.yml/badge.svg)](https://github.com/Blocra-HQ/blocra-backend/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-1.96-orange?logo=rust)
![Actix-web](https://img.shields.io/badge/Actix--web-4-blue)
![SQLite](https://img.shields.io/badge/SQLite-sqlx-003B57?logo=sqlite)
![License](https://img.shields.io/badge/license-MIT-green)

The API behind [BlocRA](https://github.com/Blocra-HQ), a blockchain analytics platform built for Stellar.
It serves live network statistics, contract analysis, saved queries and dashboards, Stellar wallet
sign-in, and an AI analyst, to the [BlocRA web app](https://github.com/Blocra-HQ/blocra-frontend).

Rust, Actix-web 4, SQLite through sqlx (migrations embedded at compile time).

---

## Contents

- [Features](#features)
- [API reference](#api-reference)
- [Data sources](#data-sources)
- [Configuration](#configuration)
- [Running locally](#running-locally)
- [Docker](#docker)
- [Deployment](#deployment)
- [Project layout](#project-layout)
- [Security](#security)
- [Known issues](#known-issues)
- [History](#history)

## Features

| Area | What it does |
|------|--------------|
| Network dashboard | Latest Stellar ledger (transactions, failures, XLM fees, operation types) from Horizon; latest Starknet block from receipts (STRK fees, gas, revert rate, transaction types) |
| Stellar sign-in | Challenge/response login with a Stellar wallet (Freighter `signMessage`, SEP-53), issuing a JWT |
| Accounts | Email/password, Google OAuth, profile and profile picture |
| AI analyst | Chat, written contract reports and natural-language-to-SQL through any OpenAI-compatible model |
| Saved queries | Store, list and reuse contract queries |
| Dashboard builder | User dashboards with widgets, positions and chart-type suggestions |
| Contract analysis | Starknet contract events and activity; EVM contract transaction history through Alchemy |
| Admin | Platform overview, user list, per-user activity and query logs |

## API reference

All routes are under `/api`. "JWT" means `Authorization: Bearer <token>` from a sign-in endpoint.

### Health

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/health` | - | Liveness check |
| GET | `/api/` | - | Welcome message |

### Authentication

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/auth/stellar/challenge` | - | `{account}` (G...) returns a one-time challenge to sign |
| POST | `/api/auth/stellar/verify` | - | `{challenge, signature}` returns `{token, user}` |
| POST | `/api/auth/wallet` | - | Removed; answers `410 Gone` (it accepted unsigned logins) |
| POST | `/api/auth/register` | - | Email/password registration |
| POST | `/api/auth/login` | - | Email/password login |
| POST | `/api/auth/google` | - | Google OAuth token exchange |
| GET | `/api/auth/oauth/config` | - | Public OAuth client settings |
| POST | `/api/auth/refresh` | - | Exchange a valid token for a new one |
| GET | `/api/auth/me` | JWT | Current user |
| GET / PUT | `/api/auth/profile` | JWT | Read or update the profile |
| POST | `/api/auth/upload-profile-image` | JWT | Multipart upload, JPEG/PNG/WebP up to 5 MB |
| POST | `/api/auth/logout` | - | Client-side logout acknowledgement |

### Network dashboards

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/dashboards/stats?chain=stellar` | - | Latest Stellar ledger statistics (default chain) |
| GET | `/api/dashboards/stats?chain=starknet` | - | Latest Starknet block statistics |
| GET | `/api/dashboards/stats?chain=<other>` | - | `501 Not Implemented` with an explanation |
| GET | `/api/dashboards` | JWT | The user's saved query and dashboard counts |
| GET | `/api/dashboards/analytics` | - | Platform totals: users, saved queries, dashboards |

Stats are computed from real chain data only. Stellar reads the whole latest ledger by following
Horizon's paging cursor; Starknet uses `starknet_getBlockWithReceipts` for actual fees and execution
status.

### AI analyst

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/ai/status` | - | Whether AI is configured, and the model |
| POST | `/api/ai/chat` | - | `{messages: [{role, content}], context?}` returns `{reply}` |
| POST | `/api/ai/report` | - | `{contract_name, contract_address, chain, stats}` returns a sectioned report |
| POST | `/api/ai/sql` | - | `{question}` returns one read-only `SELECT` |

Without `AI_BASE_URL` and `AI_MODEL` these answer `503`. Provider failures answer `502`.

### Saved queries

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET / POST | `/api/queries` | JWT | List or save queries |
| GET | `/api/queries/{id}` | JWT (owner) | One of your saved queries (`404` for others') |
| DELETE | `/api/queries/{id}` | JWT (owner) | Delete one of your saved queries |

### Dashboard builder

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET / POST | `/api/dashboard-builder` | JWT | List or create dashboards |
| GET / DELETE | `/api/dashboard-builder/{id}` | JWT | Read (with widgets) or delete a dashboard |
| POST | `/api/dashboard-builder/{id}/widgets` | JWT | Add a widget |
| PUT / DELETE | `/api/dashboard-builder/widgets/{id}` | JWT | Move/resize or remove a widget |
| GET | `/api/dashboard-builder/queries` | JWT | Queries available for widgets |
| GET | `/api/dashboard-builder/suggest/{query_id}` | JWT | Suggested chart type for a query |

### Contract analysis

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/contracts/query` | JWT | Run and record a contract query |
| GET | `/api/contracts/queries` | JWT | The user's contract queries |
| GET | `/api/contracts/queries/{id}` | JWT (owner) | One of your contract queries (`404` for others') |
| POST | `/api/contracts/events` | - | Starknet events for a contract and block range |
| POST | `/api/contracts/analyze` | - | Starknet contract activity summary |
| POST | `/api/contracts/save-query` | JWT | Save a query |
| GET | `/api/contracts/saved-queries` | JWT | Saved queries |
| POST | `/api/contract/transactions` | - | EVM contract transactions via Alchemy (Base, Ethereum, Arbitrum, Optimism, Polygon); `503` without `ALCHEMY_API_KEY` |

### Admin and feedback

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/admin/login` | `ADMIN_PASSWORD` | Admin sign-in; `503` when `ADMIN_PASSWORD` is unset or shorter than 12 characters |
| GET | `/api/admin/dashboard` | JWT, admin role | Platform overview |
| GET | `/api/admin/users` | JWT, admin role | All users |
| GET | `/api/admin/users/{user_id}` | JWT, admin role | A user's activity |
| GET | `/api/admin/query-logs` | JWT, admin role | Query logs (`?user_id=&limit=`) |
| POST | `/api/feedback/submit` | - | Submit feedback (logged; email delivery is disabled) |

Errors are JSON: `{"success": false, "message": "..."}`.

## Data sources

| Source | Used for | Endpoints |
|--------|----------|-----------|
| Stellar Horizon | Ledger, transaction and operation data | `horizon.stellar.org`, `horizon.stellar.lobstr.co` (failover in that order), or `horizon-testnet.stellar.org` |
| Starknet JSON-RPC | Blocks, receipts, events | `starknet-rpc.publicnode.com`, `starknet.api.onfinality.io/public`, `api.zan.top/public/starknet-mainnet` |
| Alchemy (optional) | EVM transaction history | Your own key |
| AI model (optional) | Chat, reports, SQL | Any OpenAI-compatible `/chat/completions` endpoint |

The public endpoints are taken from [blocra-research](https://github.com/Blocra-HQ/blocra-research),
where every RPC is checked for a live head block. Any of them can be overridden from the environment.

AI examples:

| Setup | `AI_BASE_URL` | `AI_MODEL` |
|-------|---------------|------------|
| Ollama on the same machine | `http://localhost:11434/v1` | `qwen2.5:3b` (or any pulled model) |
| vLLM serving gpt-oss | `http://<gpu-host>:8000/v1` | `openai/gpt-oss-120b` |
| Hosted provider | `https://<provider>/v1` | provider model id, plus `AI_API_KEY` |

## Configuration

Copy `.env.production` to `.env` and edit it, or set the variables in your host's dashboard.

| Variable | Default | Required | Purpose |
|----------|---------|----------|---------|
| `JWT_SECRET` | none | **yes** | Token signing key, at least 32 characters. The server exits at startup without it. Generate with `openssl rand -base64 32` |
| `JWT_EXPIRATION` | `86400` | no | Token lifetime in seconds |
| `HOST` | `0.0.0.0` | no | Bind address |
| `PORT` | `5000` | no | Bind port |
| `DATABASE_URL` | `sqlite:/tmp/blocra.db` | no | SQLite file; use persistent storage in production |
| `CORS_ORIGIN` | `*` | no | Allowed browser origin (a single origin, or `*`) |
| `STELLAR_NETWORK` | `mainnet` | no | `mainnet` or `testnet` |
| `STELLAR_HORIZON_URL` | none | no | Horizon server tried before the built-in ones |
| `STARKNET_RPC_URL` | none | no | Starknet RPC tried before the built-in ones |
| `ALCHEMY_API_KEY` | none | no | Enables `/api/contract/transactions` |
| `AI_BASE_URL` | none | no | OpenAI-compatible base URL; enables `/api/ai/*` |
| `AI_MODEL` | none | no | Model name for the AI endpoints |
| `AI_API_KEY` | none | no | Bearer token for hosted AI providers |
| `AI_TIMEOUT_SECS` | `120` | no | Timeout per AI request |
| `ADMIN_PASSWORD` | none | no | Enables `POST /api/admin/login` (at least 12 characters, compared in constant time) |
| `GOOGLE_CLIENT_ID` | none | no | Google OAuth |
| `GOOGLE_CLIENT_SECRET` | none | no | Google OAuth |
| `RATE_LIMIT_MAX_REQUESTS` | `100` | no | Read into config, not yet enforced (see [Known issues](#known-issues)) |
| `RATE_LIMIT_WINDOW_SECS` | `900` | no | Same as above |
| `RUST_LOG` | none | no | Log level, e.g. `info` |

## Running locally

Requires Rust 1.96 or newer (the version CI uses). SQLite is bundled through sqlx; no database server is needed.

```bash
git clone https://github.com/Blocra-HQ/blocra-backend.git
cd blocra-backend

export JWT_SECRET="$(openssl rand -base64 32)"
export DATABASE_URL=sqlite:./data/blocra.db
export RUST_LOG=info
# optional: export AI_BASE_URL=http://localhost:11434/v1 AI_MODEL=qwen2.5:3b

cargo run            # http://localhost:5000, migrations run on startup
```

```bash
curl http://localhost:5000/api/health
curl "http://localhost:5000/api/dashboards/stats?chain=stellar"
```

Tests and checks:

```bash
cargo test           # Stellar sign-in flow, signature checks, ledger/block summaries, AI helpers
cargo build --release
```

CI (`.github/workflows/ci.yml`) builds and tests with `--locked` on Rust 1.96 and checks that the
Docker image builds, on every push and pull request.

## Docker

```bash
docker build -t blocra-backend .
docker run -p 5000:5000 \
  -e JWT_SECRET="$(openssl rand -base64 32)" \
  -v blocra-data:/app/data -v blocra-uploads:/app/uploads \
  blocra-backend
```

The image builds on `rust:1.96` with locked dependencies and runs on `debian:bookworm-slim` as a
non-root user. SQLite (`/app/data`) and uploaded images (`/app/uploads`) live on volumes so they
survive redeploys.

## Deployment

- **Render:** `render-start.sh` refuses to start without `JWT_SECRET`. Point `DATABASE_URL` at a
  persistent disk (for example `sqlite:/var/data/blocra.db`); the `/tmp` default is wiped on every
  deploy and the server logs a warning when it detects it.
- **systemd:** `blocra-backend.service` is an example unit for a VM.
- `scripts/setup-production.sh` generates a JWT secret and prints the environment variables for the
  backend (Render) and the frontend (Vercel).

## Project layout

```
src/
  main.rs              startup: config, JWT check, DB pool, migrations, services, HTTP server
  config.rs            environment configuration
  db.rs                SQLite pool and migrations
  errors.rs            AppError and JSON error responses
  auth/
    stellar_wallet.rs  Stellar challenge/verify sign-in (SEP-53 signatures)
  routes/              HTTP routes per area (auth, dashboard, ai, contract, query, admin, ...)
  handlers/            request logic and dashboard summaries
  services/
    stellar.rs         Horizon client with failover and paging
    rpc.rs             Starknet JSON-RPC client with failover
    ai.rs              OpenAI-compatible AI client
    alchemy.rs         Alchemy transfers API
  models/              database models
  middleware/          request logging, rate-limit hook
  utils/jwt.rs         token creation and verification
migrations/            SQL migrations, applied in order at startup
scripts/               production setup helper
```

## Security

- **Wallet sign-in:** the server issues a challenge bound to one account, valid for five minutes and
  usable once (stored in SQLite). The Ed25519 signature is verified against the account key, raw or as
  a SEP-53 digest (Freighter's `signMessage`).
- **Secrets:** no keys are hardcoded. `JWT_SECRET` has no default. An Alchemy key that was committed in
  earlier history has been removed and should be revoked.
- **Uploads:** one file per request, at most 5 MB, accepted only if its bytes are JPEG, PNG or WebP;
  `/uploads` serves files by exact path with no directory listing.
- **Data integrity:** dashboard statistics are computed from chain data only; values that cannot be
  measured are omitted rather than estimated.

## Known issues

These are tracked for fixing:

- `RateLimitMiddleware` currently passes every request through; the `RATE_LIMIT_*` settings are not
  enforced.
- `CORS_ORIGIN` accepts a single origin (or `*`).

## History

- Started as the Rust rewrite of the original Node.js API, now kept read-only at
  [blocra-backend-legacy](https://github.com/Blocra-HQ/blocra-backend-legacy).
- Stellar wallet sign-in (`src/auth/stellar_wallet.rs`) and the Horizon client (`src/services/stellar.rs`)
  were imported from [stellar-insights](https://github.com/Ndifreke000/stellar-insights) with their
  commit history, then adapted (SQLite instead of Redis, a slimmed client).
- The bounty API was removed. Its tables (migrations 002 to 005) remain so existing databases keep
  migrating; sqlx refuses to start if an applied migration file disappears.

## Related repositories

| Repository | Role |
|------------|------|
| [blocra-frontend](https://github.com/Blocra-HQ/blocra-frontend) | Web app (React, TypeScript, Vite) |
| [blocra-contracts](https://github.com/Blocra-HQ/blocra-contracts) | Soroban report registry on Stellar |
| [blocra-research](https://github.com/Blocra-HQ/blocra-research) | Verified chain/RPC registry and checker |
| [blocra-backend-legacy](https://github.com/Blocra-HQ/blocra-backend-legacy) | Original Node.js API, read-only |

## License

[MIT](LICENSE)
