//! Background collection of Stellar ledgers into the analytics store.
//!
//! Every few seconds the collector reads the ledgers closed since the last
//! one stored and saves each with its transactions and operations, so the
//! dashboard has a complete per-ledger history and SQL queries see every
//! ledger rather than whichever one a page happened to poll.
//!
//! On first start it backfills ANALYTICS_STELLAR_BACKFILL ledgers (default 30,
//! about two and a half minutes of chain time). If it falls further behind than MAX_BACKLOG ledgers
//! (Horizon outage, server asleep) it skips ahead to stay current and logs
//! the gap. Disable with ANALYTICS_COLLECT=false.
//!
//! Each ledger's contract events come from Stellar RPC (getEvents) at the
//! same time. If the RPC fails, the ledger is stored without events and the
//! day's coverage records it; ANALYTICS_EVENTS=false turns events off.

use futures_util::StreamExt;
use std::time::Duration;

use crate::handlers::dashboard::summarize_ledger;
use crate::services::analytics::{self, Analytics};
use crate::services::contract_events::{decode_event, DecodedEvent};
use crate::services::soroban_rpc::SorobanRpc;
use crate::services::stellar::StellarService;

const TICK: Duration = Duration::from_secs(3);
const MAX_BACKLOG: u64 = 120;
/// Ledgers fetched concurrently while catching up.
const PARALLEL: usize = 6;
const PRUNE_EVERY: Duration = Duration::from_secs(600);

pub fn enabled() -> bool {
    !matches!(std::env::var("ANALYTICS_COLLECT").as_deref(), Ok("false") | Ok("0"))
}

fn events_enabled() -> bool {
    !matches!(std::env::var("ANALYTICS_EVENTS").as_deref(), Ok("false") | Ok("0"))
}

pub fn spawn(analytics: Analytics, stellar: StellarService) {
    let rpc = events_enabled().then(SorobanRpc::new);
    if rpc.is_none() {
        log::warn!("ANALYTICS_EVENTS=false; contract events are not collected");
    }
    actix_web::rt::spawn(async move {
        let backfill: u64 = std::env::var("ANALYTICS_STELLAR_BACKFILL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let mut last_prune = std::time::Instant::now() - PRUNE_EVERY;
        log::info!("Stellar ledger collector started ({} network)", stellar.network);
        loop {
            if let Err(e) = tick(&analytics, &stellar, rpc.as_ref(), backfill).await {
                log::warn!("Stellar collector: {e}");
            }
            if last_prune.elapsed() >= PRUNE_EVERY {
                if let Err(e) = analytics.prune().await {
                    log::warn!("Analytics prune failed: {e}");
                }
                last_prune = std::time::Instant::now();
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// The decoded contract events of a ledger, or `None` if the RPC failed.
async fn ledger_events(rpc: Option<&SorobanRpc>, ledger: u64) -> Option<Vec<DecodedEvent>> {
    let rpc = rpc?;
    match rpc.events_in_ledger(ledger).await {
        Ok(events) => Some(events.iter().filter_map(decode_event).collect()),
        Err(e) => {
            log::warn!("Stellar collector: events of ledger {ledger} unavailable: {e}");
            None
        }
    }
}

async fn tick(analytics: &Analytics, stellar: &StellarService, rpc: Option<&SorobanRpc>, backfill: u64) -> Result<(), String> {
    let mut latest = stellar.latest_ledger().await.map_err(|e| e.to_string())?.sequence;
    // Stay behind the RPC too, so each ledger's events are already there.
    if let Some(rpc) = rpc {
        match rpc.latest_ledger().await {
            Ok(rpc_latest) => latest = latest.min(rpc_latest),
            Err(e) => log::warn!("Stellar collector: RPC latest ledger unavailable: {e}"),
        }
    }
    let stored = analytics.latest_block("stellar").await.map_err(|e| e.to_string())?;
    let mut after = match stored {
        Some(s) if s as u64 >= latest => return Ok(()),
        Some(s) if latest - s as u64 > MAX_BACKLOG => {
            log::warn!(
                "Stellar collector is {} ledgers behind; skipping to the last {MAX_BACKLOG}",
                latest - s as u64
            );
            latest - MAX_BACKLOG
        }
        Some(s) => s as u64,
        None => latest.saturating_sub(backfill),
    };

    while after < latest {
        let ledgers = stellar
            .ledgers_after(after, (latest - after).min(50) as u32)
            .await
            .map_err(|e| e.to_string())?;
        if ledgers.is_empty() {
            break;
        }
        // Fetch a few ledgers at once (each needs several Horizon pages),
        // then store them in order.
        let mut fetched = futures_util::stream::iter(ledgers.into_iter().map(|ledger| async move {
            let (pages, events) = tokio::join!(
                async {
                    tokio::try_join!(
                        stellar.ledger_transactions(ledger.sequence),
                        stellar.ledger_operations(ledger.sequence),
                    )
                },
                ledger_events(rpc, ledger.sequence),
            );
            (ledger, pages, events)
        }))
        .buffered(PARALLEL);
        while let Some((ledger, pages, events)) = fetched.next().await {
            let (txs, ops) = pages.map_err(|e| format!("ledger {}: {e}", ledger.sequence))?;
            let summary = summarize_ledger(&ledger, &txs, &ops, &stellar.network);
            analytics
                .record_stellar_ledger(&summary, &txs, &ops, events.as_deref())
                .await
                .map_err(|e| format!("storing ledger {}: {e}", ledger.sequence))?;
            after = ledger.sequence;
        }
    }
    log::debug!("Stellar collector at ledger {after} ({}s behind clock)", lag(analytics).await);
    Ok(())
}

async fn lag(analytics: &Analytics) -> i64 {
    match analytics.latest_summary("stellar").await {
        Ok(Some((s, _))) => analytics::now() - s["timestamp"].as_i64().unwrap_or(0),
        _ => -1,
    }
}
