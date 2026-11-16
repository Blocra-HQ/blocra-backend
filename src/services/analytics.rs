//! BlocRA's analytics store: on-chain data the backend has collected, kept in
//! its own SQLite database so it can be queried with SQL.
//!
//! - `blocks`: one row per stored block or ledger, for every chain. Stellar
//!   ledgers are all collected by `services::collector`; other chains are
//!   stored whenever their dashboard stats are fetched.
//! - `transactions` and `operations`: Stellar, one row per record.
//! - `block_summaries`: the full /api/dashboards/stats response per block,
//!   which serves the dashboard's history and its cached stats.
//! - `events` and `token_transfers`: contract events of Soroban transactions
//!   from Stellar RPC, raw; `event_daily`, `token_flow_daily` and
//!   `contract_reach_daily` keep their daily totals.
//!
//! The database holds only public chain data (never users or sessions) and
//! user SQL runs on a separate read-only connection pool, so a query can't
//! modify or reach anything else.
//!
//! ANALYTICS_DATABASE_URL picks the file (default: `analytics.db` next to
//! DATABASE_URL). ANALYTICS_TX_RETENTION_HOURS (default 24),
//! ANALYTICS_EVENT_RETENTION_HOURS (default 72) and
//! ANALYTICS_BLOCK_RETENTION_DAYS (default 30) bound its size; daily tables
//! are kept.

use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, SqlitePool, TypeInfo, ValueRef};
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::errors::AppError;
use crate::services::contract_events::DecodedEvent;
use crate::services::stellar::{HorizonOperation, HorizonTransaction};
use std::collections::{HashMap, HashSet};

/// Rows a query may return.
pub const MAX_ROWS: usize = 1000;
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS blocks (
    chain TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    transaction_count INTEGER,
    successful_txs INTEGER,
    failed_txs INTEGER,
    unique_senders INTEGER,
    operation_count INTEGER,
    total_fees REAL,
    fee_unit TEXT,
    gas_used INTEGER,
    PRIMARY KEY (chain, block_number)
);
CREATE INDEX IF NOT EXISTS blocks_by_time ON blocks (chain, timestamp);

CREATE TABLE IF NOT EXISTS transactions (
    chain TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    hash TEXT NOT NULL,
    sender TEXT,
    fee REAL,
    fee_unit TEXT,
    operation_count INTEGER,
    successful INTEGER,
    PRIMARY KEY (chain, hash)
);
CREATE INDEX IF NOT EXISTS transactions_by_block ON transactions (chain, block_number);
CREATE INDEX IF NOT EXISTS transactions_by_sender ON transactions (sender);

CREATE TABLE IF NOT EXISTS operations (
    chain TEXT NOT NULL,
    id TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    tx_hash TEXT NOT NULL,
    type TEXT NOT NULL,
    source TEXT,
    PRIMARY KEY (chain, id)
);
CREATE INDEX IF NOT EXISTS operations_by_block ON operations (chain, block_number);
CREATE INDEX IF NOT EXISTS operations_by_type ON operations (type);

-- Daily activity per subject (a Soroban contract C..., or an asset XLM /
-- CODE:ISSUER) and account, kept long-term for traction reports.
CREATE TABLE IF NOT EXISTS activity_daily (
    subject TEXT NOT NULL,
    day TEXT NOT NULL,
    account TEXT NOT NULL,
    kind TEXT NOT NULL,
    calls INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    volume REAL NOT NULL DEFAULT 0,
    self_calls INTEGER NOT NULL DEFAULT 0,
    relayed_calls INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (subject, day, account)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS activity_by_day ON activity_daily (day, kind);

CREATE TABLE IF NOT EXISTS function_daily (
    contract TEXT NOT NULL,
    day TEXT NOT NULL,
    function TEXT NOT NULL,
    calls INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (contract, day, function)
) WITHOUT ROWID;

-- Days whose ledgers were collected, with how many, so reports can state coverage.
CREATE TABLE IF NOT EXISTS collected_days (
    day TEXT PRIMARY KEY,
    ledgers INTEGER NOT NULL DEFAULT 0,
    first_ledger INTEGER,
    last_ledger INTEGER,
    first_ts INTEGER,
    last_ts INTEGER
);

-- Contract events (raw, recent hours only) from Soroban transactions.
CREATE TABLE IF NOT EXISTS events (
    chain TEXT NOT NULL,
    id TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    tx_hash TEXT NOT NULL,
    op_index INTEGER,
    contract TEXT NOT NULL,
    name TEXT,
    topics TEXT NOT NULL,
    data TEXT,
    user_account TEXT,
    PRIMARY KEY (chain, id)
);
CREATE INDEX IF NOT EXISTS events_by_contract ON events (contract, block_number);

-- Token movements read from transfer/mint/burn/clawback events (raw, recent hours only).
CREATE TABLE IF NOT EXISTS token_transfers (
    chain TEXT NOT NULL,
    id TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    tx_hash TEXT NOT NULL,
    token TEXT NOT NULL,
    kind TEXT NOT NULL,
    from_address TEXT,
    to_address TEXT,
    amount TEXT NOT NULL,
    amount_raw REAL NOT NULL,
    asset TEXT,
    user_account TEXT,
    PRIMARY KEY (chain, id)
);
CREATE INDEX IF NOT EXISTS token_transfers_by_token ON token_transfers (token, block_number);
CREATE INDEX IF NOT EXISTS token_transfers_by_from ON token_transfers (from_address);
CREATE INDEX IF NOT EXISTS token_transfers_by_to ON token_transfers (to_address);

-- Events per contract, name and day (kept).
CREATE TABLE IF NOT EXISTS event_daily (
    contract TEXT NOT NULL,
    day TEXT NOT NULL,
    name TEXT NOT NULL,
    events INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (contract, day, name)
) WITHOUT ROWID;

-- Tokens in and out of each contract or liquidity pool per day, in the token's smallest unit (kept).
CREATE TABLE IF NOT EXISTS token_flow_daily (
    token TEXT NOT NULL,
    day TEXT NOT NULL,
    address TEXT NOT NULL,
    received REAL NOT NULL DEFAULT 0,
    sent REAL NOT NULL DEFAULT 0,
    transfers_in INTEGER NOT NULL DEFAULT 0,
    transfers_out INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (token, day, address)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS token_flow_by_address ON token_flow_daily (address, day);

-- Users of transactions that touched a contract, directly or through other contracts (kept).
CREATE TABLE IF NOT EXISTS contract_reach_daily (
    contract TEXT NOT NULL,
    day TEXT NOT NULL,
    account TEXT NOT NULL,
    txs INTEGER NOT NULL DEFAULT 0,
    direct_txs INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (contract, day, account)
) WITHOUT ROWID;

-- Contract interfaces read from the chain (cache).
CREATE TABLE IF NOT EXISTS contract_interfaces (
    contract TEXT PRIMARY KEY,
    kind TEXT,
    wasm_hash TEXT,
    interface TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS block_summaries (
    chain TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    collected_at INTEGER NOT NULL,
    summary TEXT NOT NULL,
    PRIMARY KEY (chain, block_number)
);
";

/// Documented tables and columns, served by GET /api/query/schema and used
/// in the AI's text-to-SQL prompt.
/// (name, type, description) of a column.
pub type ColumnDoc = (&'static str, &'static str, &'static str);

pub const TABLES: &[(&str, &str, &[ColumnDoc])] = &[
    (
        "blocks",
        "One row per stored block or ledger. Every Stellar ledger is collected; other chains are sampled whenever their dashboard is viewed.",
        &[
            ("chain", "TEXT", "Chain id, e.g. stellar, ethereum, solana"),
            ("block_number", "INTEGER", "Block height or ledger sequence"),
            ("timestamp", "INTEGER", "Close time, unix seconds (use datetime(timestamp, 'unixepoch'))"),
            ("transaction_count", "INTEGER", "Transactions in the block, failed included"),
            ("successful_txs", "INTEGER", "Successful transactions"),
            ("failed_txs", "INTEGER", "Failed or reverted transactions"),
            ("unique_senders", "INTEGER", "Distinct source accounts / senders"),
            ("operation_count", "INTEGER", "Stellar operations applied in the ledger, i.e. from successful transactions only (the operations table also has failed ones); null on other chains"),
            ("total_fees", "REAL", "Fees paid in the block, in fee_unit"),
            ("fee_unit", "TEXT", "XLM, ETH, SOL, ..."),
            ("gas_used", "INTEGER", "Gas (EVM, Starknet) or compute units (Solana); null on Stellar"),
        ],
    ),
    (
        "transactions",
        "Stellar transactions from every collected ledger (recent hours only; see retention).",
        &[
            ("chain", "TEXT", "stellar"),
            ("block_number", "INTEGER", "Ledger sequence"),
            ("timestamp", "INTEGER", "Ledger close time, unix seconds"),
            ("hash", "TEXT", "Transaction hash"),
            ("sender", "TEXT", "Source account (G...)"),
            ("fee", "REAL", "Fee charged, in fee_unit"),
            ("fee_unit", "TEXT", "XLM"),
            ("operation_count", "INTEGER", "Operations in the transaction"),
            ("successful", "INTEGER", "1 if it succeeded, 0 if it failed"),
        ],
    ),
    (
        "operations",
        "Stellar operations from every collected ledger, including those in failed transactions (recent hours only; see retention).",
        &[
            ("chain", "TEXT", "stellar"),
            ("id", "TEXT", "Operation id"),
            ("block_number", "INTEGER", "Ledger sequence"),
            ("timestamp", "INTEGER", "Ledger close time, unix seconds"),
            ("tx_hash", "TEXT", "Transaction hash"),
            ("type", "TEXT", "payment, invoke_host_function, manage_sell_offer, path_payment_strict_send, ..."),
            ("source", "TEXT", "Operation source account (for relayed contract calls, the relayer)"),
            ("contract", "TEXT", "Contract called (invoke_host_function), C..."),
            ("function", "TEXT", "Contract function called"),
            ("user_account", "TEXT", "Contract calls: the account that authorized the call (G... or a smart-wallet C...)"),
            ("relayed", "INTEGER", "Contract calls: 1 if submitted by an account other than the user (relayer, sponsor)"),
            ("asset", "TEXT", "Payments: asset moved, XLM or CODE:ISSUER"),
            ("amount", "REAL", "Payments: amount in asset units"),
            ("destination", "TEXT", "Payments: receiving account"),
            ("args", "TEXT", "Contract calls: arguments after the function name, as a JSON array (use json_extract(args, '$[0]'))"),
        ],
    ),
    (
        "events",
        "Contract events emitted by Soroban transactions, including those from contracts called by other contracts (recent hours only; see retention). Fee events are left out.",
        &[
            ("chain", "TEXT", "stellar"),
            ("id", "TEXT", "Event id (ordered within the ledger)"),
            ("block_number", "INTEGER", "Ledger sequence"),
            ("timestamp", "INTEGER", "Ledger close time, unix seconds"),
            ("tx_hash", "TEXT", "Transaction hash"),
            ("op_index", "INTEGER", "Operation index in the transaction"),
            ("contract", "TEXT", "Contract that emitted the event, C..."),
            ("name", "TEXT", "First topic when it is a symbol or string, e.g. transfer, swap, mint"),
            ("topics", "TEXT", "All topics as a JSON array (128-bit numbers as strings, addresses as strkeys)"),
            ("data", "TEXT", "Event data as JSON (use json_extract(data, '$.amount'))"),
            ("user_account", "TEXT", "The transaction's user: first authorizer of its contract call, else its source account"),
        ],
    ),
    (
        "token_transfers",
        "Token movements read from transfer, mint, burn and clawback events in Soroban transactions (recent hours only; see retention).",
        &[
            ("chain", "TEXT", "stellar"),
            ("id", "TEXT", "Event id"),
            ("block_number", "INTEGER", "Ledger sequence"),
            ("timestamp", "INTEGER", "Ledger close time, unix seconds"),
            ("tx_hash", "TEXT", "Transaction hash"),
            ("token", "TEXT", "Token contract, C..."),
            ("kind", "TEXT", "transfer, mint, burn or clawback"),
            ("from_address", "TEXT", "Sender (null for mint)"),
            ("to_address", "TEXT", "Recipient (null for burn and clawback)"),
            ("amount", "TEXT", "Exact amount in the token's smallest unit"),
            ("amount_raw", "REAL", "Same amount as a number (may round past 15 digits); divide by 10^decimals (7 for Stellar assets)"),
            ("asset", "TEXT", "Stellar Asset Contracts: native or CODE:ISSUER"),
            ("user_account", "TEXT", "The transaction's user"),
        ],
    ),
    (
        "event_daily",
        "Events per contract, event name and UTC day (kept).",
        &[
            ("contract", "TEXT", "Contract, C..."),
            ("day", "TEXT", "UTC day, YYYY-MM-DD"),
            ("name", "TEXT", "Event name ((unnamed) when the first topic is not text)"),
            ("events", "INTEGER", "Events emitted"),
        ],
    ),
    (
        "token_flow_daily",
        "Tokens received and sent by each contract or liquidity pool per UTC day, from token events (kept). received - sent over time is the change in what the contract holds.",
        &[
            ("token", "TEXT", "Token contract, C..."),
            ("day", "TEXT", "UTC day, YYYY-MM-DD"),
            ("address", "TEXT", "Contract (C...) or liquidity pool (L...) holding the token"),
            ("received", "REAL", "Amount received or minted to it, smallest unit"),
            ("sent", "REAL", "Amount sent, burned or clawed back from it, smallest unit"),
            ("transfers_in", "INTEGER", "Movements in"),
            ("transfers_out", "INTEGER", "Movements out"),
        ],
    ),
    (
        "contract_reach_daily",
        "Users per contract and UTC day, counting every Soroban transaction that touched the contract: called it, made it emit an event, or moved its tokens, directly or through other contracts (kept).",
        &[
            ("contract", "TEXT", "Contract, C..."),
            ("day", "TEXT", "UTC day, YYYY-MM-DD"),
            ("account", "TEXT", "The transaction's user (G... or a smart-wallet C...)"),
            ("txs", "INTEGER", "Transactions that touched the contract"),
            ("direct_txs", "INTEGER", "Of those, transactions that called it directly"),
        ],
    ),
];

/// Schema in the compact form the AI prompt uses.
pub fn schema_for_prompt() -> String {
    TABLES
        .iter()
        .map(|(name, _, cols)| {
            let cols: Vec<String> = cols.iter().map(|(c, t, _)| format!("{c} {t}")).collect();
            format!("{name}({})", cols.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone)]
pub struct Analytics {
    rw: SqlitePool,
    ro: SqlitePool,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Map<String, Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `sqlite:<dir>/analytics.db` next to the main database unless
/// ANALYTICS_DATABASE_URL is set.
pub fn database_url(main_url: &str) -> String {
    if let Ok(url) = std::env::var("ANALYTICS_DATABASE_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    let path = main_url.strip_prefix("sqlite:").unwrap_or(main_url);
    let path = path.split('?').next().unwrap_or(path);
    let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    if dir.is_empty() {
        "sqlite:analytics.db?mode=rwc".to_string()
    } else {
        format!("sqlite:{dir}/analytics.db?mode=rwc")
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn int(v: &Value) -> Option<i64> {
    num(v).map(|f| f as i64)
}

fn parse_time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).map(|t| t.timestamp()).unwrap_or(0)
}

impl Analytics {
    pub async fn open(url: &str) -> Result<Self, sqlx::Error> {
        let rw_opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let rw = SqlitePoolOptions::new().max_connections(2).connect_with(rw_opts.clone()).await?;
        sqlx::raw_sql(SCHEMA).execute(&rw).await?;
        // Columns added after the first release; ignore "duplicate column".
        for col in ["contract TEXT", "function TEXT", "asset TEXT", "amount REAL", "destination TEXT", "user_account TEXT", "relayed INTEGER", "args TEXT"] {
            let _ = sqlx::query(&format!("ALTER TABLE operations ADD COLUMN {col}")).execute(&rw).await;
        }
        let _ = sqlx::query("ALTER TABLE activity_daily ADD COLUMN self_calls INTEGER NOT NULL DEFAULT 0").execute(&rw).await;
        let _ = sqlx::query("ALTER TABLE activity_daily ADD COLUMN relayed_calls INTEGER NOT NULL DEFAULT 0").execute(&rw).await;
        for col in ["first_ledger INTEGER", "last_ledger INTEGER", "first_ts INTEGER", "last_ts INTEGER", "event_ledgers INTEGER NOT NULL DEFAULT 0"] {
            let _ = sqlx::query(&format!("ALTER TABLE collected_days ADD COLUMN {col}")).execute(&rw).await;
        }
        sqlx::raw_sql("CREATE INDEX IF NOT EXISTS operations_by_contract ON operations (contract)")
            .execute(&rw)
            .await?;
        let ro_opts = rw_opts.read_only(true).create_if_missing(false).pragma("query_only", "ON");
        let ro = SqlitePoolOptions::new().max_connections(4).connect_with(ro_opts).await?;
        Ok(Self { rw, ro })
    }

    /// Stores a block from its /stats summary (any chain). Re-storing the
    /// same block is a no-op.
    pub async fn record_summary(&self, chain: &str, summary: &Value) -> Result<(), sqlx::Error> {
        let mut tx = self.rw.begin().await?;
        insert_block(&mut tx, chain, summary).await?;
        tx.commit().await
    }

    /// Stores a Stellar ledger with its transactions, operations and contract
    /// events, and adds it to the daily tables. `events` is `None` when they
    /// could not be fetched (the day's `event_ledgers` then shows the gap).
    pub async fn record_stellar_ledger(
        &self,
        summary: &Value,
        txs: &[HorizonTransaction],
        ops: &[HorizonOperation],
        events: Option<&[DecodedEvent]>,
    ) -> Result<(), sqlx::Error> {
        let ledger = summary["block_number"].as_i64().unwrap_or(0);
        let ts = summary["timestamp"].as_i64().unwrap_or(0);
        let day = day_of(ts);
        let mut tx = self.rw.begin().await?;
        // A ledger stored before is not counted twice in the daily tables.
        if !insert_block(&mut tx, "stellar", summary).await? {
            return tx.commit().await;
        }
        for t in txs {
            let fee = t.fee_charged.as_deref().and_then(|f| f.parse::<f64>().ok()).map(|s| s / 1e7);
            sqlx::query(
                "INSERT OR IGNORE INTO transactions
                 (chain, block_number, timestamp, hash, sender, fee, fee_unit, operation_count, successful)
                 VALUES ('stellar', ?, ?, ?, ?, ?, 'XLM', ?, ?)",
            )
            .bind(ledger)
            .bind(ts)
            .bind(&t.hash)
            .bind(&t.source_account)
            .bind(fee)
            .bind(t.operation_count as i64)
            .bind(t.successful as i64)
            .execute(&mut *tx)
            .await?;
        }
        // Authorization entries per transaction, decoded only for contract calls.
        let envelopes: HashMap<&str, &str> = txs
            .iter()
            .filter_map(|t| Some((t.hash.as_str(), t.envelope_xdr.as_deref()?)))
            .collect();
        // Soroban transactions and their users, for attributing events.
        let soroban_txs: HashSet<&str> =
            ops.iter().filter(|o| o.operation_type == "invoke_host_function").map(|o| o.transaction_hash.as_str()).collect();
        let tx_users: HashMap<&str, String> = txs
            .iter()
            .filter(|t| soroban_txs.contains(t.hash.as_str()))
            .map(|t| {
                let auth = t.envelope_xdr.as_deref().map(crate::services::soroban_auth::authorizers).unwrap_or_default();
                (t.hash.as_str(), auth.into_iter().next().unwrap_or_else(|| t.source_account.clone()))
            })
            .collect();
        // (contract, tx) pairs already counted in contract_reach_daily, and
        // which of them were direct calls.
        let mut reached: HashMap<(String, String), bool> = HashMap::new();
        for o in ops {
            let op_ts = parse_time(&o.created_at);
            let call = o.contract_call();
            // The user of a contract call is the first account that authorized
            // it; without authorization entries it is the operation's source.
            // A call whose source didn't authorize it was relayed.
            let (user, relayed) = match &call {
                Some(_) => {
                    let auth = envelopes
                        .get(o.transaction_hash.as_str())
                        .map(|e| crate::services::soroban_auth::authorizers(e))
                        .unwrap_or_default();
                    match auth.first() {
                        Some(first) => (first.clone(), !auth.contains(&o.source_account)),
                        None => (o.source_account.clone(), false),
                    }
                }
                None => (o.source_account.clone(), false),
            };
            let asset = o.payment_asset();
            let amount = o.amount.as_deref().and_then(|a| a.parse::<f64>().ok());
            let failed = !o.transaction_successful.unwrap_or(true);
            let args = call.as_ref().map(|_| o.call_args().to_string());
            sqlx::query(
                "INSERT OR IGNORE INTO operations
                 (chain, id, block_number, timestamp, tx_hash, type, source, contract, function, asset, amount, destination,
                  user_account, relayed, args)
                 VALUES ('stellar', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&o.id)
            .bind(ledger)
            .bind(if op_ts > 0 { op_ts } else { ts })
            .bind(&o.transaction_hash)
            .bind(&o.operation_type)
            .bind(&o.source_account)
            .bind(call.as_ref().map(|c| c.0.as_str()))
            .bind(call.as_ref().and_then(|c| c.1.as_deref()))
            .bind(asset.as_deref())
            .bind(if asset.is_some() { amount } else { None })
            .bind(o.to.as_deref())
            .bind(call.as_ref().map(|_| user.as_str()))
            .bind(call.as_ref().map(|_| relayed as i64))
            .bind(args)
            .execute(&mut *tx)
            .await?;

            if let (Some((contract, _)), false) = (&call, failed) {
                reached.insert((contract.clone(), o.transaction_hash.clone()), true);
            }
            if let Some((contract, function)) = &call {
                add_activity(&mut tx, Activity { subject: contract, day: &day, account: &user, kind: "contract", failed, volume: 0.0, to_self: false, relayed }).await?;
                sqlx::query(
                    "INSERT INTO function_daily (contract, day, function, calls, failed) VALUES (?, ?, ?, 1, ?)
                     ON CONFLICT (contract, day, function) DO UPDATE SET
                       calls = calls + 1, failed = failed + excluded.failed",
                )
                .bind(contract)
                .bind(&day)
                .bind(function.as_deref().unwrap_or("(unknown)"))
                .bind(failed as i64)
                .execute(&mut *tx)
                .await?;
            }
            if let Some(asset) = &asset {
                // Payments move value only when the transaction succeeded, and
                // a payment to oneself (arbitrage loops via path payments)
                // moves none to anyone else, so neither counts as volume.
                let sender = o.from.as_deref().unwrap_or(&o.source_account);
                let to_self = o.to.as_deref() == Some(sender);
                let volume = if failed || to_self { 0.0 } else { amount.unwrap_or(0.0) };
                add_activity(&mut tx, Activity { subject: asset, day: &day, account: sender, kind: "asset", failed, volume, to_self, relayed: false }).await?;
            }
        }
        for e in events.unwrap_or_default() {
            // Classic payments emit token events too (CAP-67); those are
            // already counted from Horizon, so only Soroban transactions here.
            let Some(user) = tx_users.get(e.tx_hash.as_str()) else { continue };
            let name = e.name.as_deref().unwrap_or("(unnamed)");
            sqlx::query(
                "INSERT OR IGNORE INTO events (chain, id, block_number, timestamp, tx_hash, op_index, contract, name, topics, data, user_account)
                 VALUES ('stellar', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&e.id)
            .bind(ledger)
            .bind(ts)
            .bind(&e.tx_hash)
            .bind(e.op_index as i64)
            .bind(&e.contract)
            .bind(e.name.as_deref())
            .bind(Value::Array(e.topics.clone()).to_string())
            .bind(e.data.to_string())
            .bind(user)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO event_daily (contract, day, name, events) VALUES (?, ?, ?, 1)
                 ON CONFLICT (contract, day, name) DO UPDATE SET events = events + 1",
            )
            .bind(&e.contract)
            .bind(&day)
            .bind(name)
            .execute(&mut *tx)
            .await?;
            reached.entry((e.contract.clone(), e.tx_hash.clone())).or_insert(false);

            let Some(m) = &e.token_move else { continue };
            sqlx::query(
                "INSERT OR IGNORE INTO token_transfers
                 (chain, id, block_number, timestamp, tx_hash, token, kind, from_address, to_address, amount, amount_raw, asset, user_account)
                 VALUES ('stellar', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&e.id)
            .bind(ledger)
            .bind(ts)
            .bind(&e.tx_hash)
            .bind(&e.contract)
            .bind(m.kind)
            .bind(m.from.as_deref())
            .bind(m.to.as_deref())
            .bind(m.amount.to_string())
            .bind(m.amount as f64)
            .bind(m.asset.as_deref())
            .bind(user)
            .execute(&mut *tx)
            .await?;
            // Holdings of contracts and liquidity pools; accounts are left out
            // to keep the table small.
            for (holder, received) in [(m.to.as_deref(), true), (m.from.as_deref(), false)] {
                let Some(holder) = holder.filter(|h| h.starts_with('C') || h.starts_with('L')) else { continue };
                let (r, s_) = if received { (m.amount as f64, 0.0) } else { (0.0, m.amount as f64) };
                sqlx::query(
                    "INSERT INTO token_flow_daily (token, day, address, received, sent, transfers_in, transfers_out)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT (token, day, address) DO UPDATE SET
                       received = received + excluded.received, sent = sent + excluded.sent,
                       transfers_in = transfers_in + excluded.transfers_in, transfers_out = transfers_out + excluded.transfers_out",
                )
                .bind(&e.contract)
                .bind(&day)
                .bind(holder)
                .bind(r)
                .bind(s_)
                .bind(received as i64)
                .bind(!received as i64)
                .execute(&mut *tx)
                .await?;
                if holder.starts_with('C') {
                    reached.entry((holder.to_string(), e.tx_hash.clone())).or_insert(false);
                }
            }
        }
        for ((contract, tx_hash), direct) in &reached {
            let Some(user) = tx_users.get(tx_hash.as_str()) else { continue };
            sqlx::query(
                "INSERT INTO contract_reach_daily (contract, day, account, txs, direct_txs) VALUES (?, ?, ?, 1, ?)
                 ON CONFLICT (contract, day, account) DO UPDATE SET txs = txs + 1, direct_txs = direct_txs + excluded.direct_txs",
            )
            .bind(contract)
            .bind(&day)
            .bind(user)
            .bind(*direct as i64)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "INSERT INTO collected_days (day, ledgers, first_ledger, last_ledger, first_ts, last_ts, event_ledgers) VALUES (?, 1, ?, ?, ?, ?, ?)
             ON CONFLICT (day) DO UPDATE SET ledgers = ledgers + 1, event_ledgers = event_ledgers + excluded.event_ledgers,
               first_ledger = MIN(COALESCE(first_ledger, excluded.first_ledger), excluded.first_ledger),
               last_ledger = MAX(COALESCE(last_ledger, excluded.last_ledger), excluded.last_ledger),
               first_ts = MIN(COALESCE(first_ts, excluded.first_ts), excluded.first_ts),
               last_ts = MAX(COALESCE(last_ts, excluded.last_ts), excluded.last_ts)",
        )
        .bind(&day)
        .bind(ledger)
        .bind(ledger)
        .bind(ts)
        .bind(ts)
        .bind(events.is_some() as i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// A contract interface read before, and when (unix seconds).
    pub async fn cached_interface(&self, contract: &str) -> Result<Option<(Value, i64)>, sqlx::Error> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT interface, fetched_at FROM contract_interfaces WHERE contract = ?")
                .bind(contract)
                .fetch_optional(&self.rw)
                .await?;
        Ok(row.and_then(|(i, at)| serde_json::from_str(&i).ok().map(|v| (v, at))))
    }

    pub async fn store_interface(&self, contract: &str, interface: &Value) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO contract_interfaces (contract, kind, wasm_hash, interface, fetched_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (contract) DO UPDATE SET kind = excluded.kind, wasm_hash = excluded.wasm_hash,
               interface = excluded.interface, fetched_at = excluded.fetched_at",
        )
        .bind(contract)
        .bind(interface["kind"].as_str())
        .bind(interface["wasm_hash"].as_str())
        .bind(interface.to_string())
        .bind(now())
        .execute(&self.rw)
        .await?;
        Ok(())
    }

    /// The newest events a contract emitted (optionally one event name), with
    /// the token movement each one is, if any.
    pub async fn contract_events(&self, contract: &str, name: Option<&str>, limit: i64) -> Result<Vec<Value>, sqlx::Error> {
        let rows: Vec<EventRow> =
            sqlx::query_as(
                "SELECT e.id, e.block_number, e.timestamp, e.tx_hash, e.name, e.topics, e.data, e.user_account,
                        t.kind, t.from_address, t.to_address, t.amount, t.asset
                 FROM events e LEFT JOIN token_transfers t ON t.chain = e.chain AND t.id = e.id
                 WHERE e.contract = ? AND (? IS NULL OR e.name = ?)
                 ORDER BY e.block_number DESC, e.id DESC LIMIT ?",
            )
            .bind(contract)
            .bind(name)
            .bind(name)
            .bind(limit.clamp(1, 500))
            .fetch_all(&self.rw)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, ledger, ts, tx, name, topics, data, user, kind, from, to, amount, asset)| {
                json!({
                    "id": id, "ledger": ledger, "timestamp": ts, "tx_hash": tx, "name": name,
                    "topics": serde_json::from_str::<Value>(&topics).unwrap_or(Value::Null),
                    "data": data.and_then(|d| serde_json::from_str::<Value>(&d).ok()).unwrap_or(Value::Null),
                    "user_account": user,
                    "token_move": kind.map(|k| json!({ "kind": k, "from": from, "to": to, "amount": amount, "asset": asset })),
                })
            })
            .collect())
    }

    /// The newest direct calls to a contract, with their arguments.
    pub async fn contract_calls(&self, contract: &str, limit: i64) -> Result<Vec<Value>, sqlx::Error> {
        let rows: Vec<CallRow> = sqlx::query_as(
            "SELECT o.id, o.block_number, o.timestamp, o.tx_hash, o.function, o.args, o.user_account, o.relayed, t.successful
             FROM operations o LEFT JOIN transactions t ON t.chain = o.chain AND t.hash = o.tx_hash
             WHERE o.contract = ? ORDER BY o.block_number DESC, o.id DESC LIMIT ?",
        )
        .bind(contract)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.rw)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, ledger, ts, tx, function, args, user, relayed, ok)| {
                json!({
                    "id": id, "ledger": ledger, "timestamp": ts, "tx_hash": tx, "function": function,
                    "args": args.and_then(|a| serde_json::from_str::<Value>(&a).ok()).unwrap_or(json!([])),
                    "user_account": user, "relayed": relayed.map(|r| r == 1),
                    "successful": ok.map(|s| s == 1),
                })
            })
            .collect())
    }

    /// Daily activity of a contract over the last `days` UTC days: direct
    /// callers, everyone it reached (through other contracts too), its events
    /// and the tokens it received and sent.
    pub async fn contract_activity(&self, contract: &str, days: i64) -> Result<Value, sqlx::Error> {
        let days = days.clamp(1, 90);
        let since = day_of(now() - (days - 1) * 86_400);
        let direct: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT day, COUNT(*), SUM(calls), SUM(failed) FROM activity_daily
             WHERE subject = ? AND kind = 'contract' AND day >= ? GROUP BY day ORDER BY day",
        )
        .bind(contract)
        .bind(&since)
        .fetch_all(&self.rw)
        .await?;
        let reach: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT day, COUNT(*), SUM(direct_txs > 0), SUM(txs), SUM(direct_txs) FROM contract_reach_daily
             WHERE contract = ? AND day >= ? GROUP BY day ORDER BY day",
        )
        .bind(contract)
        .bind(&since)
        .fetch_all(&self.rw)
        .await?;
        let totals: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(DISTINCT account), COUNT(DISTINCT CASE WHEN direct_txs > 0 THEN account END)
             FROM contract_reach_daily WHERE contract = ? AND day >= ?",
        )
        .bind(contract)
        .bind(&since)
        .fetch_one(&self.rw)
        .await?;
        let events: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT day, name, events FROM event_daily WHERE contract = ? AND day >= ? ORDER BY day, events DESC",
        )
        .bind(contract)
        .bind(&since)
        .fetch_all(&self.rw)
        .await?;
        let flows: Vec<(String, String, f64, f64, i64, i64)> = sqlx::query_as(
            "SELECT day, token, received, sent, transfers_in, transfers_out FROM token_flow_daily
             WHERE address = ? AND day >= ? ORDER BY day, token",
        )
        .bind(contract)
        .bind(&since)
        .fetch_all(&self.rw)
        .await?;
        let coverage: (Option<i64>, Option<i64>) =
            sqlx::query_as("SELECT SUM(ledgers), SUM(event_ledgers) FROM collected_days WHERE day >= ?")
                .bind(&since)
                .fetch_one(&self.rw)
                .await?;
        Ok(json!({
            "contract": contract,
            "since": since,
            "days": days,
            "users_reached": totals.0,
            "users_direct": totals.1,
            "users_indirect_only": totals.0 - totals.1,
            "direct_calls": direct.iter().map(|(d, users, calls, failed)| json!({ "day": d, "users": users, "calls": calls, "failed": failed })).collect::<Vec<_>>(),
            "reach": reach.iter().map(|(d, users, direct_users, txs, direct_txs)| json!({
                "day": d, "users": users, "direct_users": direct_users, "indirect_only_users": users - direct_users,
                "txs": txs, "direct_txs": direct_txs,
            })).collect::<Vec<_>>(),
            "events": events.iter().map(|(d, n, c)| json!({ "day": d, "name": n, "events": c })).collect::<Vec<_>>(),
            "token_flows": flows.iter().map(|(d, t, r, s, i, o)| json!({
                "day": d, "token": t, "received": r, "sent": s, "net": r - s, "transfers_in": i, "transfers_out": o,
            })).collect::<Vec<_>>(),
            "coverage": { "ledgers": coverage.0.unwrap_or(0), "ledgers_with_events": coverage.1.unwrap_or(0) },
        }))
    }

    /// The pool reports and other read-only internal queries use.
    pub fn reader(&self) -> &SqlitePool {
        &self.rw
    }

    pub async fn latest_block(&self, chain: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar("SELECT MAX(block_number) FROM blocks WHERE chain = ?")
            .bind(chain)
            .fetch_one(&self.rw)
            .await
    }

    /// The newest stored summary and when it was collected.
    pub async fn latest_summary(&self, chain: &str) -> Result<Option<(Value, i64)>, sqlx::Error> {
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT summary, collected_at FROM block_summaries WHERE chain = ? ORDER BY block_number DESC LIMIT 1",
        )
        .bind(chain)
        .fetch_optional(&self.rw)
        .await?;
        Ok(row.and_then(|(s, at)| serde_json::from_str(&s).ok().map(|v| (v, at))))
    }

    /// The last `limit` stored summaries for a chain, oldest first.
    pub async fn history(&self, chain: &str, limit: i64) -> Result<Vec<Value>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT summary FROM (SELECT block_number, summary FROM block_summaries WHERE chain = ?
             ORDER BY block_number DESC LIMIT ?) ORDER BY block_number ASC",
        )
        .bind(chain)
        .bind(limit)
        .fetch_all(&self.rw)
        .await?;
        Ok(rows.into_iter().filter_map(|(s,)| serde_json::from_str(&s).ok()).collect())
    }

    /// Stored blocks per chain and the time range they cover.
    pub async fn coverage(&self) -> Result<Vec<Value>, sqlx::Error> {
        let rows: Vec<(String, i64, i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT chain, COUNT(*), MIN(block_number), MAX(block_number), MIN(timestamp), MAX(timestamp)
             FROM blocks GROUP BY chain ORDER BY COUNT(*) DESC",
        )
        .fetch_all(&self.rw)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(chain, blocks, first, last, t0, t1)| {
                json!({ "chain": chain, "blocks": blocks, "first_block": first, "last_block": last,
                        "first_time": t0, "last_time": t1 })
            })
            .collect())
    }

    /// Deletes data past its retention period.
    pub async fn prune(&self) -> Result<(), sqlx::Error> {
        let env = |k: &str, d: i64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let tx_cutoff = now() - env("ANALYTICS_TX_RETENTION_HOURS", 24) * 3600;
        let block_cutoff = now() - env("ANALYTICS_BLOCK_RETENTION_DAYS", 30) * 86_400;
        let event_cutoff = now() - env("ANALYTICS_EVENT_RETENTION_HOURS", 72) * 3600;
        sqlx::query("DELETE FROM events WHERE timestamp < ?").bind(event_cutoff).execute(&self.rw).await?;
        sqlx::query("DELETE FROM token_transfers WHERE timestamp < ?").bind(event_cutoff).execute(&self.rw).await?;
        sqlx::query("DELETE FROM transactions WHERE timestamp < ?").bind(tx_cutoff).execute(&self.rw).await?;
        sqlx::query("DELETE FROM operations WHERE timestamp < ?").bind(tx_cutoff).execute(&self.rw).await?;
        sqlx::query("DELETE FROM blocks WHERE timestamp < ?").bind(block_cutoff).execute(&self.rw).await?;
        sqlx::query("DELETE FROM block_summaries WHERE collected_at < ?")
            .bind(block_cutoff)
            .execute(&self.rw)
            .await?;
        Ok(())
    }

    /// Runs one read-only SELECT (or WITH ... SELECT) and returns at most
    /// MAX_ROWS rows.
    pub async fn run_query(&self, sql: &str) -> Result<QueryResult, AppError> {
        let sql = validate_query(sql)?;
        let wrapped = format!("SELECT * FROM ({sql}\n) LIMIT {}", MAX_ROWS + 1);
        let started = Instant::now();
        let rows = tokio::time::timeout(QUERY_TIMEOUT, sqlx::query(&wrapped).fetch_all(&self.ro))
            .await
            .map_err(|_| AppError::BadRequest(format!("Query took longer than {}s", QUERY_TIMEOUT.as_secs())))?
            .map_err(|e| AppError::BadRequest(sql_error(&e)))?;
        let elapsed_ms = started.elapsed().as_millis();
        let columns: Vec<String> = rows
            .first()
            .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default();
        let truncated = rows.len() > MAX_ROWS;
        let rows: Vec<Map<String, Value>> = rows.iter().take(MAX_ROWS).map(row_to_json).collect();
        Ok(QueryResult { row_count: rows.len(), columns, rows, truncated, elapsed_ms })
    }
}

/// Returns whether the block was new.
async fn insert_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    chain: &str,
    summary: &Value,
) -> Result<bool, sqlx::Error> {
    let block = match summary["block_number"].as_i64() {
        Some(b) => b,
        None => return Ok(false),
    };
    let s = &summary["stats"];
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO blocks (chain, block_number, timestamp, transaction_count, successful_txs,
         failed_txs, unique_senders, operation_count, total_fees, fee_unit, gas_used)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(chain)
    .bind(block)
    .bind(summary["timestamp"].as_i64().unwrap_or(0))
    .bind(int(&s["total_transactions"]))
    .bind(int(&s["successful_txs"]))
    .bind(int(&s["failed_txs"]))
    .bind(int(&s["unique_senders"]))
    .bind(int(&s["operations"]))
    .bind(num(&s["total_fees"]))
    .bind(s["fee_unit"].as_str())
    .bind(int(&s["gas_used"]))
    .execute(&mut **tx)
    .await?
    .rows_affected()
        > 0;
    sqlx::query("INSERT OR IGNORE INTO block_summaries (chain, block_number, collected_at, summary) VALUES (?, ?, ?, ?)")
        .bind(chain)
        .bind(block)
        .bind(now())
        .bind(summary.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(inserted)
}

/// id, ledger, timestamp, tx, name, topics, data, user, then the token
/// movement: kind, from, to, amount, asset.
type EventRow = (
    String, i64, i64, String, Option<String>, String, Option<String>, Option<String>,
    Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
);
/// id, ledger, timestamp, tx, function, args, user, relayed, successful.
type CallRow = (String, i64, i64, String, Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>);

struct Activity<'a> {
    subject: &'a str,
    day: &'a str,
    account: &'a str,
    kind: &'a str,
    failed: bool,
    volume: f64,
    to_self: bool,
    relayed: bool,
}

async fn add_activity(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, a: Activity<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO activity_daily (subject, day, account, kind, calls, failed, volume, self_calls, relayed_calls)
         VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?)
         ON CONFLICT (subject, day, account) DO UPDATE SET
           calls = calls + 1, failed = failed + excluded.failed, volume = volume + excluded.volume,
           self_calls = self_calls + excluded.self_calls, relayed_calls = relayed_calls + excluded.relayed_calls",
    )
    .bind(a.subject)
    .bind(a.day)
    .bind(a.account)
    .bind(a.kind)
    .bind(a.failed as i64)
    .bind(a.volume)
    .bind(a.to_self as i64)
    .bind(a.relayed as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// UTC calendar day of a unix timestamp, e.g. "2026-11-06".
pub fn day_of(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.date_naive().to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

/// Checks that `sql` is a single SELECT/WITH statement and returns it without
/// a trailing semicolon. The read-only connection is the real guard; this
/// gives clear errors and keeps the LIMIT wrapper valid.
pub fn validate_query(sql: &str) -> Result<String, AppError> {
    let mut s = sql.trim();
    while let Some(rest) = s.strip_suffix(';') {
        s = rest.trim_end();
    }
    if s.is_empty() {
        return Err(AppError::BadRequest("Query is empty".into()));
    }
    if s.len() > 20_000 {
        return Err(AppError::BadRequest("Query is too long (20,000 characters max)".into()));
    }
    let code = strip_literals_and_comments(s);
    if code.contains(';') {
        return Err(AppError::BadRequest("Only one statement per query".into()));
    }
    let first = code.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if first != "select" && first != "with" {
        return Err(AppError::BadRequest("Only SELECT queries are allowed (SELECT ... or WITH ... SELECT ...)".into()));
    }
    let lower = code.to_ascii_lowercase();
    for word in ["attach", "detach", "pragma", "vacuum", "insert", "update", "delete", "drop", "create", "alter"] {
        if lower.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|w| w == word) {
            return Err(AppError::BadRequest(format!("'{}' is not allowed; queries are read-only", word.to_uppercase())));
        }
    }
    Ok(s.to_string())
}

/// Blanks out string literals and comments so keyword checks only see code.
fn strip_literals_and_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                let quote = c;
                out.push(' ');
                while let Some(n) = chars.next() {
                    if n == quote {
                        if chars.peek() == Some(&quote) {
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

fn sql_error(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Database(db) => format!("SQL error: {}", db.message()),
        other => format!("SQL error: {other}"),
    }
}

fn row_to_json(row: &SqliteRow) -> Map<String, Value> {
    let mut map = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        let value = match row.try_get_raw(i) {
            Ok(raw) if raw.is_null() => Value::Null,
            Ok(raw) => match raw.type_info().name() {
                "INTEGER" => row.try_get::<i64, _>(i).map(Value::from).unwrap_or(Value::Null),
                "REAL" => row.try_get::<f64, _>(i).map(Value::from).unwrap_or(Value::Null),
                "BLOB" => row
                    .try_get::<Vec<u8>, _>(i)
                    .map(|b| Value::from(b.iter().map(|x| format!("{x:02x}")).collect::<String>()))
                    .unwrap_or(Value::Null),
                _ => row.try_get::<String, _>(i).map(Value::from).unwrap_or(Value::Null),
            },
            Err(_) => Value::Null,
        };
        map.insert(col.name().to_string(), value);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_queries() {
        assert_eq!(validate_query(" SELECT 1 ;; ").unwrap(), "SELECT 1");
        assert!(validate_query("WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
        assert!(validate_query("select ';' as s, 'drop' as d -- delete\n").is_ok());
        for bad in [
            "",
            "DELETE FROM blocks",
            "SELECT 1; DROP TABLE blocks",
            "PRAGMA table_info(blocks)",
            "ATTACH 'x.db' AS x",
            "SELECT * FROM blocks; SELECT 2",
            "WITH x AS (SELECT 1) DELETE FROM blocks",
        ] {
            assert!(validate_query(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn derives_the_database_path() {
        std::env::remove_var("ANALYTICS_DATABASE_URL");
        assert_eq!(database_url("sqlite:/var/data/blocra.db?mode=rwc"), "sqlite:/var/data/analytics.db?mode=rwc");
        assert_eq!(database_url("sqlite:blocra.db"), "sqlite:analytics.db?mode=rwc");
    }

    async fn store() -> (Analytics, tempdir::TempDirGuard) {
        let dir = tempdir::TempDirGuard::new();
        let url = format!("sqlite:{}/a.db?mode=rwc", dir.0.display());
        (Analytics::open(&url).await.unwrap(), dir)
    }

    fn summary(chain: &str, block: i64, ts: i64, txs: i64) -> Value {
        json!({ "success": true, "chain": chain, "block_number": block, "timestamp": ts,
                "stats": { "total_transactions": txs, "successful_txs": txs - 1, "failed_txs": 1,
                           "unique_senders": 3, "total_fees": "0.25", "fee_unit": "XLM", "operations": 9 } })
    }

    #[tokio::test]
    async fn stores_history_and_answers_sql() {
        let (a, _dir) = store().await;
        for b in 1..=5 {
            a.record_summary("stellar", &summary("stellar", b, 1_000 + b * 5, 10 * b)).await.unwrap();
        }
        a.record_summary("stellar", &summary("stellar", 3, 0, 0)).await.unwrap(); // duplicate ignored
        let h = a.history("stellar", 3).await.unwrap();
        assert_eq!(h.iter().map(|v| v["block_number"].as_i64().unwrap()).collect::<Vec<_>>(), vec![3, 4, 5]);
        assert_eq!(a.latest_block("stellar").await.unwrap(), Some(5));

        let r = a
            .run_query("SELECT chain, SUM(transaction_count) AS txs, ROUND(SUM(total_fees), 2) AS fees FROM blocks GROUP BY chain")
            .await
            .unwrap();
        assert_eq!(r.columns, vec!["chain", "txs", "fees"]);
        assert_eq!(r.rows[0]["txs"], json!(150));
        assert_eq!(r.rows[0]["fees"], json!(1.25));

        // Read-only: writes fail even if they slip past validation.
        assert!(a.run_query("SELECT * FROM blocks WHERE 1 = (SELECT 1)").await.is_ok());
        assert!(a.run_query("SELECT nope FROM blocks").await.is_err());
        let cov = a.coverage().await.unwrap();
        assert_eq!(cov[0]["blocks"], json!(5));
    }

    #[tokio::test]
    async fn read_only_pool_rejects_writes() {
        let (a, _dir) = store().await;
        let err = sqlx::query("INSERT INTO blocks (chain, block_number, timestamp) VALUES ('x', 1, 1)")
            .execute(&a.ro)
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn caps_rows() {
        let (a, _dir) = store().await;
        let r = a
            .run_query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 1500) SELECT x FROM n")
            .await
            .unwrap();
        assert_eq!(r.row_count, MAX_ROWS);
        assert!(r.truncated);
    }

    #[tokio::test]
    async fn records_events_flows_and_indirect_reach() {
        use crate::services::contract_events::{DecodedEvent, TokenMove};
        use crate::services::stellar::{HorizonOperation, HostFunctionParam};
        let (a, _dir) = store().await;
        let router = crate::services::scval::address("AAAAEgAAAAHtzg5P6SASi0gXcoiUXbIyNil74ihUeLn00E5zwvYBPg==").unwrap();
        let pool = "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA";
        let token = "CDL74RF5BLYR2YBLCCI7F5FB6TPSCLKEJUBSD2RSVWZ4YHF3VMFAIGWA";
        let user = "GBQ6QIHVDHFSY4CDORDELIZXRUWF72FTCRNOXUFPH7YOO3PFFMXLAVUN";
        let ts = now();
        let tx = |hash: &str| HorizonTransaction {
            hash: hash.into(), paging_token: "1".into(), ledger: 7, created_at: String::new(),
            source_account: user.into(), fee_account: None, fee_charged: Some("100".into()), max_fee: None,
            operation_count: 1, successful: true, envelope_xdr: None,
        };
        let call = HorizonOperation {
            id: "1".into(), transaction_hash: "soroban".into(), source_account: user.into(),
            operation_type: "invoke_host_function".into(), transaction_successful: Some(true),
            function: Some("HostFunctionTypeHostFunctionTypeInvokeContract".into()),
            parameters: Some(vec![
                HostFunctionParam { param_type: "Address".into(), value: "AAAAEgAAAAHtzg5P6SASi0gXcoiUXbIyNil74ihUeLn00E5zwvYBPg==".into() },
                HostFunctionParam { param_type: "Sym".into(), value: "AAAADwAAAAlzZXRfcHJpY2UAAAA=".into() },
                HostFunctionParam { param_type: "I128".into(), value: "AAAACgAAAAAAAAAAAAAAAAAAAGQ=".into() },
            ]),
            ..Default::default()
        };
        let payment = HorizonOperation {
            id: "2".into(), transaction_hash: "classic".into(), source_account: user.into(),
            operation_type: "payment".into(), transaction_successful: Some(true), asset_type: Some("native".into()),
            from: Some(user.into()), to: Some(pool.into()), amount: Some("1".into()), ..Default::default()
        };
        let ev = |id: &str, tx: &str, contract: &str, name: &str, m: Option<TokenMove>| DecodedEvent {
            id: id.into(), tx_hash: tx.into(), op_index: 0, contract: contract.into(),
            name: Some(name.into()), topics: vec![json!(name)], data: json!("100"), token_move: m,
        };
        let moved = |amount| Some(TokenMove { kind: "transfer", from: Some(user.into()), to: Some(pool.into()), amount, asset: None });
        let events = vec![
            ev("e1", "soroban", pool, "swap", None),
            ev("e2", "soroban", token, "transfer", moved(100)),
            ev("e3", "classic", token, "transfer", moved(5)), // classic payment: counted from Horizon, not here
        ];
        let summary = json!({ "block_number": 7, "timestamp": ts, "stats": {} });
        for _ in 0..2 {
            // The second time is a re-delivery of the same ledger and must not double count.
            a.record_stellar_ledger(&summary, &[tx("soroban"), tx("classic")], &[call.clone(), payment.clone()], Some(&events)).await.unwrap();
        }
        let q = |sql: &str| {
            let a = a.clone();
            let sql = sql.to_string();
            async move { a.run_query(&sql).await.unwrap().rows }
        };
        assert_eq!(q("SELECT COUNT(*) AS n FROM events").await[0]["n"], json!(2));
        assert_eq!(q("SELECT user_account AS u FROM events LIMIT 1").await[0]["u"], json!(user));
        let flows = q("SELECT address, received, transfers_in FROM token_flow_daily").await;
        assert_eq!(flows.len(), 1);
        assert_eq!((flows[0]["address"].clone(), flows[0]["received"].clone()), (json!(pool), json!(100.0)));
        assert_eq!(q("SELECT args FROM operations WHERE contract IS NOT NULL").await[0]["args"], json!("[\"100\"]"));
        let reach = q("SELECT contract, txs, direct_txs FROM contract_reach_daily ORDER BY contract").await;
        let by = |c: &str| reach.iter().find(|r| r["contract"] == c).cloned().unwrap();
        assert_eq!((by(&router)["txs"].clone(), by(&router)["direct_txs"].clone()), (json!(1), json!(1)));
        assert_eq!((by(pool)["txs"].clone(), by(pool)["direct_txs"].clone()), (json!(1), json!(0)), "pool reached through the router");
        assert_eq!(by(token)["direct_txs"], json!(0));
        assert_eq!(q("SELECT event_ledgers FROM collected_days").await[0]["event_ledgers"], json!(1));

        let act = a.contract_activity(pool, 7).await.unwrap();
        assert_eq!(act["users_indirect_only"], json!(1));
        assert_eq!(act["token_flows"][0]["net"], json!(100.0));
        let listed = a.contract_events(token, None, 10).await.unwrap();
        assert_eq!(listed[0]["token_move"]["amount"], json!("100"));
        assert_eq!(a.contract_calls(&router, 10).await.unwrap()[0]["args"], json!(["100"]));
    }

    mod tempdir {
        pub struct TempDirGuard(pub std::path::PathBuf);
        impl TempDirGuard {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!(
                    "blocra-analytics-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
                ));
                std::fs::create_dir_all(&p).unwrap();
                Self(p)
            }
        }
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
