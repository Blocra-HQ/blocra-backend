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
| Network dashboard | Latest block statistics for 37 chains: Stellar (Horizon), Starknet, 31 EVM chains, Solana and three CometBFT chains (transactions, failures, fees, gas, senders, transaction types), with stored history |
| Analytics store | Every Stellar ledger collected in the background (with its transactions and operations) plus every block a dashboard fetches, in a separate SQLite database |
| SQL | Read-only SQL over the analytics store for the Query Editor and Dashboard Builder |
| Traction reports | 7- or 30-day reports on a Soroban contract or Stellar asset: active, new and returning accounts, failure rate, concentration and bot share, function breakdown, ecosystem rank, coverage and methodology |
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
| GET | `/api/dashboards/stats?chain=<id>` | - | Latest block statistics for any chain in the registry; unknown ids return `404` with the supported list, upstream failures `502` |
| GET | `/api/dashboards/history?chain=<id>&limit=60` | - | Stored blocks for a chain, oldest first, each in the `/stats` shape (`limit` 1-500) |
| GET | `/api/dashboards` | JWT | The user's saved query and dashboard counts |
| GET | `/api/dashboards/analytics` | - | Platform totals: users, saved queries, dashboards |

Stats are computed from real chain data only. Stellar reads the whole latest ledger by following
Horizon's paging cursor; Starknet uses `starknet_getBlockWithReceipts` for actual fees and execution
status.

Every other chain comes from the registry in `src/config/chains.json` (copied from
[blocra-research](https://github.com/Blocra-HQ/blocra-research), embedded at build time). Each
chain's RPCs are tried in order with a 15 s timeout; `RPC_URL_<ID>` (e.g. `RPC_URL_ETHEREUM`) is
tried first when set. Endpoints whose head block is more than an hour old are skipped.

| Type | Chains | Source | Notes |
|------|--------|--------|-------|
| `evm` | ethereum, base, arbitrum, optimism, polygon, bnb, avalanche, zksync, linea, scroll, zora, mantle, unichain, taiko, cronos, okxchain, aurora, gnosis, celo, berachain, sei, kava, metis, flare, songbird, fuse, chiliz, pulsechain, haqq, syscoin, xpla | `eth_getBlockByNumber` and `eth_getBlockReceipts` (per-transaction receipts where unsupported) | Fees are gas used x effective gas price, plus the rollup L1 fee and blob fee when the receipt reports them. Celo transactions paying fees in other tokens are counted in `non_native_fee_txs` and left out of `total_fees`. `tx_types` are envelope types; `tx_kinds` splits transfers, calls and contract creations |
| `solana` | solana | `getSlot`, `getBlock` (confirmed) | Fees from `meta.fee`, failures from `meta.err`, senders are fee payers, `gas_used` is compute units; `tx_types` are message versions |
| `cometbft` | axelar, celer, saga | `block`, `block_results` | Status and gas from `txs_results`; fees from the `tx` event's `fee` attribute in the chain's native denomination; `tx_types` are first message types |

The response shape is the same for every chain. A figure that cannot be read exactly is `null`
and `stats.partial` says why (for example `"receipts unavailable"`); nothing is estimated.

Every block `/stats` fetches is stored, and a block stored less than 8 seconds ago is served from the
store instead of the RPC. Stellar ledgers are all collected in the background (next section), so
Stellar stats answer in milliseconds and its history has every ledger; other chains' history holds
the blocks that were fetched while someone had their dashboard open.

### Analytics store and SQL

The backend keeps the chain data it collects in its own SQLite database (`ANALYTICS_DATABASE_URL`,
default `analytics.db` next to the main database). It holds public chain data only, never accounts.

- **Collector** (`src/services/collector.rs`): every 3 s it reads the Stellar ledgers closed since
  the last one stored (Horizon, 6 ledgers fetched at a time) and stores each ledger with all of its
  transactions and operations. On first start it backfills `ANALYTICS_STELLAR_BACKFILL` ledgers. If it
  falls more than 120 ledgers behind it skips ahead and logs the gap.
- **Retention**: transactions and operations are kept `ANALYTICS_TX_RETENTION_HOURS` (24); blocks
  `ANALYTICS_BLOCK_RETENTION_DAYS` (30). Stellar mainnet produces about 260 transactions and 500
  operations per ledger (one ledger every ~5 s).

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/query/schema` | - | Tables, columns and stored coverage per chain |
| POST | `/api/query/run` | - | `{sql}`: one read-only `SELECT`/`WITH` query, at most 1,000 rows, 10 s timeout. Returns `{columns, rows, row_count, truncated, elapsed_ms}` |

| Table | Rows | Columns |
|-------|------|---------|
| `blocks` | Every stored block or ledger, all chains | `chain, block_number, timestamp, transaction_count, successful_txs, failed_txs, unique_senders, operation_count, total_fees, fee_unit, gas_used` |
| `transactions` | Stellar transactions | `chain, block_number, timestamp, hash, sender, fee, fee_unit, operation_count, successful` |
| `operations` | Stellar operations | `chain, id, block_number, timestamp, tx_hash, type, source` |

Queries run on a separate read-only connection pool (`query_only`), so they cannot write even if
they get past validation, and the analytics database contains no user data to reach. Example:

```sql
SELECT strftime('%H:%M', timestamp, 'unixepoch') AS minute,
       SUM(transaction_count) AS txs, SUM(failed_txs) AS failed, ROUND(SUM(total_fees), 4) AS fees_xlm
FROM blocks WHERE chain = 'stellar'
GROUP BY minute ORDER BY minute;
```

### Traction reports

A traction report measures one Soroban contract (`C...`) or asset (`XLM`, `CODE:ISSUER`) over a 7- or
30-day period against the period before, from the collector's daily tables (`activity_daily`,
`function_daily`, `collected_days`). Every figure is computed in SQL; the findings are rule-based
sentences over those figures; nothing is estimated or written by a model.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/reports/traction?subject=<id>&days=7` | - | The report (`days` 1-90). `404` when the subject has no recorded activity |
| GET | `/api/reports/subjects?kind=contract\|asset&days=7&limit=20` | - | Most active contracts or assets, for pickers |

| Section | Contents |
|---------|----------|
| `kpis` | Active accounts, new accounts (marked unreliable when history is shorter than the period), calls or payments, failure rate, volume (assets), calls per account, each with the previous period and % change |
| `daily` | Active accounts, calls, failures and volume per UTC day |
| `retention` | Accounts active in both periods |
| `quality` | Top-10 account share of calls, accounts with at least 1,000 calls on one day (automated) and their share, top 5 accounts |
| `functions` | Calls and failure rate per contract function |
| `ecosystem` | Rank by active accounts among all contracts (or assets) active in the period, and the peers' median failure rate |
| `asset_snapshot` | Holders and supply (accounts, contracts, pools) from Horizon at generation time |
| `coverage` | Ledgers aggregated in the period, ledger range, completeness and days covered; reports say plainly when data is partial |
| `methodology` | Definitions behind every figure |

Contract calls are decoded from each `invoke_host_function` operation's parameters (contract address
and function symbol, XDR in `src/services/scval.rs`). Asset activity is payments and path payments by
sender; volume sums successful payments.

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
| GET / POST | `/api/dashboard-builder` | JWT | List or create dashboards (`{name, description, widgets?}`; a widget carries `query_sql` or a `data` snapshot) |
| GET / PUT / DELETE | `/api/dashboard-builder/{id}` | JWT | Read (with widgets), rename and replace widgets, or delete a dashboard |
| POST | `/api/dashboard-builder/{id}/widgets` | JWT | Add a widget |
| PUT / DELETE | `/api/dashboard-builder/widgets/{id}` | JWT | Move/resize or remove a widget |
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
| EVM, Solana and CometBFT JSON-RPC | Latest block, receipts and execution results for registry chains | Public endpoints listed per chain in `src/config/chains.json` |
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
| `CORS_ORIGIN` | `*` | no | Allowed browser origins, comma-separated, or `*` |
| `STELLAR_NETWORK` | `mainnet` | no | `mainnet` or `testnet` |
| `STELLAR_HORIZON_URL` | none | no | Horizon server tried before the built-in ones |
| `STARKNET_RPC_URL` | none | no | Starknet RPC tried before the built-in ones |
| `RPC_URL_<ID>` | none | no | RPC tried before the registry's endpoints for chain `<id>`, e.g. `RPC_URL_POLYGON` |
| `ALCHEMY_API_KEY` | none | no | Enables `/api/contract/transactions` |
| `AI_BASE_URL` | none | no | OpenAI-compatible base URL; enables `/api/ai/*` |
| `AI_MODEL` | `gpt-oss:120b-cloud` on Ollama, else `openai/gpt-oss-120b` | no | Model name for the AI endpoints |
| `AI_REASONING_EFFORT` | `low` | no | `low`, `medium` or `high` for gpt-oss |
| `AI_API_KEY` | none | no | Bearer token for hosted AI providers |
| `AI_TIMEOUT_SECS` | `120` | no | Timeout per AI request |
| `ADMIN_PASSWORD` | none | no | Enables `POST /api/admin/login` (at least 12 characters, compared in constant time) |
| `GOOGLE_CLIENT_ID` | none | no | Google OAuth |
| `GOOGLE_CLIENT_SECRET` | none | no | Google OAuth |
| `ANALYTICS_DATABASE_URL` | `analytics.db` next to `DATABASE_URL` | no | SQLite file for collected chain data |
| `ANALYTICS_COLLECT` | `true` | no | Set `false` to stop collecting Stellar ledgers |
| `ANALYTICS_STELLAR_BACKFILL` | `30` | no | Ledgers backfilled on first start |
| `ANALYTICS_TX_RETENTION_HOURS` | `24` | no | How long Stellar transactions and operations are kept |
| `ANALYTICS_BLOCK_RETENTION_DAYS` | `30` | no | How long blocks and dashboard history are kept |
| `RATE_LIMIT_MAX_REQUESTS` | `1000` | no | Requests allowed per client IP per window (`429` with `Retry-After` beyond it; `/api/health` is exempt) |
| `RATE_LIMIT_WINDOW_SECS` | `900` | no | Rate-limit window in seconds |
| `TRUST_PROXY` | unset | no | Set to `1` behind a reverse proxy (Render, nginx) so rate limits use the client IP from `X-Forwarded-For` |
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
    chains.rs          chain registry and generic JSON-RPC client with failover
  config/chains.json   chain registry (RPCs, currency, type) embedded at build time
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

- Per-transaction tables (`transactions`, `operations`) are collected for Stellar only; other
  chains contribute block-level rows when their dashboard is viewed.
- The analytics store is SQLite: fine for one server, not for several replicas writing at once.

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
