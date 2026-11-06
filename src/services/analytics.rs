//! BlocRA's analytics store: on-chain data the backend has collected, kept in
//! its own SQLite database so it can be queried with SQL.
//!
//! - `blocks`: one row per stored block or ledger, for every chain. Stellar
//!   ledgers are all collected by `services::collector`; other chains are
//!   stored whenever their dashboard stats are fetched.
//! - `transactions` and `operations`: Stellar, one row per record.
//! - `block_summaries`: the full /api/dashboards/stats response per block,
//!   which serves the dashboard's history and its cached stats.
//!
//! The database holds only public chain data (never users or sessions) and
//! user SQL runs on a separate read-only connection pool, so a query can't
//! modify or reach anything else.
//!
//! ANALYTICS_DATABASE_URL picks the file (default: `analytics.db` next to
//! DATABASE_URL). ANALYTICS_TX_RETENTION_HOURS (default 24) and
//! ANALYTICS_BLOCK_RETENTION_DAYS (default 30) bound its size.

use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, SqlitePool, TypeInfo, ValueRef};
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::errors::AppError;
use crate::services::stellar::{HorizonOperation, HorizonTransaction};

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
        for col in ["contract TEXT", "function TEXT", "asset TEXT", "amount REAL", "destination TEXT", "user_account TEXT", "relayed INTEGER"] {
            let _ = sqlx::query(&format!("ALTER TABLE operations ADD COLUMN {col}")).execute(&rw).await;
        }
        let _ = sqlx::query("ALTER TABLE activity_daily ADD COLUMN self_calls INTEGER NOT NULL DEFAULT 0").execute(&rw).await;
        let _ = sqlx::query("ALTER TABLE activity_daily ADD COLUMN relayed_calls INTEGER NOT NULL DEFAULT 0").execute(&rw).await;
        for col in ["first_ledger INTEGER", "last_ledger INTEGER", "first_ts INTEGER", "last_ts INTEGER"] {
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

    /// Stores a Stellar ledger with its transactions and operations, and adds
    /// it to the daily activity tables.
    pub async fn record_stellar_ledger(
        &self,
        summary: &Value,
        txs: &[HorizonTransaction],
        ops: &[HorizonOperation],
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
        let envelopes: std::collections::HashMap<&str, &str> = txs
            .iter()
            .filter_map(|t| Some((t.hash.as_str(), t.envelope_xdr.as_deref()?)))
            .collect();
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
            sqlx::query(
                "INSERT OR IGNORE INTO operations
                 (chain, id, block_number, timestamp, tx_hash, type, source, contract, function, asset, amount, destination,
                  user_account, relayed)
                 VALUES ('stellar', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            .execute(&mut *tx)
            .await?;

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
        sqlx::query(
            "INSERT INTO collected_days (day, ledgers, first_ledger, last_ledger, first_ts, last_ts) VALUES (?, 1, ?, ?, ?, ?)
             ON CONFLICT (day) DO UPDATE SET ledgers = ledgers + 1,
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
        .execute(&mut *tx)
        .await?;
        tx.commit().await
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
