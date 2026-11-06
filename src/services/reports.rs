//! Traction reports for Stellar projects: a Soroban contract or an asset,
//! over a 7- or 30-day period compared with the period before.
//!
//! Every figure is computed here from the analytics store's daily activity
//! tables (filled by the ledger collector); the findings are rule-based
//! sentences over those figures, so the report never states a number that
//! wasn't measured. Coverage is reported alongside: the store only knows
//! ledgers collected since the collector started.

use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::errors::AppError;
use crate::services::analytics::{day_of, now, Analytics};
use crate::services::stellar::StellarService;

/// Accounts making at least this many calls in one day are flagged as
/// automated ("bot-like"). Humans rarely sign 1,000 transactions a day.
pub const BOT_CALLS_PER_DAY: i64 = 1000;
/// Stellar mainnet closes a ledger roughly every 5.5 s.
const LEDGERS_PER_DAY: f64 = 86_400.0 / 5.5;

#[derive(Debug, Clone, Copy)]
struct Period<'a> {
    from: &'a str,
    to: &'a str,
}

fn shift_day(day: &str, delta: i64) -> String {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| (d + chrono::Duration::days(delta)).to_string())
        .unwrap_or_else(|_| day.to_string())
}

fn change_pct(now: f64, before: f64) -> Value {
    if before == 0.0 {
        Value::Null
    } else {
        json!(((now - before) / before * 1000.0).round() / 10.0)
    }
}

fn kpi(value: f64, previous: Option<f64>) -> Value {
    match previous {
        Some(p) => json!({ "value": value, "previous": p, "change_pct": change_pct(value, p) }),
        None => json!({ "value": value, "previous": null, "change_pct": null }),
    }
}

struct Totals {
    active: i64,
    calls: i64,
    failed: i64,
    volume: f64,
    self_calls: i64,
    relayed_calls: i64,
    smart_wallets: i64,
}

/// A Stellar account (G...) or contract (C...) id: base32, 56 characters.
pub fn is_strkey(s: &str) -> bool {
    s.len() == 56
        && matches!(s.as_bytes()[0], b'G' | b'C')
        && s.bytes().all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
}

/// " AND account NOT IN ('G...', ...)" for declared relayers. Ids are
/// validated as strkeys first, so inlining them is safe.
fn exclusion(relayers: &[String]) -> String {
    if relayers.is_empty() {
        return String::new();
    }
    let list: Vec<String> = relayers.iter().map(|r| format!("'{r}'")).collect();
    format!(" AND account NOT IN ({})", list.join(", "))
}

async fn totals(db: &SqlitePool, subject: &str, p: Period<'_>, excl: &str) -> Result<Totals, sqlx::Error> {
    type TotalsRow = (i64, Option<i64>, Option<i64>, Option<f64>, Option<i64>, Option<i64>, i64);
    let (active, calls, failed, volume, self_calls, relayed_calls, smart_wallets): TotalsRow = sqlx::query_as(&format!(
        "SELECT COUNT(DISTINCT account), SUM(calls), SUM(failed), SUM(volume), SUM(self_calls), SUM(relayed_calls),
                COUNT(DISTINCT CASE WHEN account LIKE 'C%' THEN account END)
         FROM activity_daily WHERE subject = ? AND day BETWEEN ? AND ?{excl}",
    ))
    .bind(subject)
    .bind(p.from)
    .bind(p.to)
    .fetch_one(db)
    .await?;
    Ok(Totals {
        active,
        calls: calls.unwrap_or(0),
        failed: failed.unwrap_or(0),
        volume: volume.unwrap_or(0.0),
        self_calls: self_calls.unwrap_or(0),
        relayed_calls: relayed_calls.unwrap_or(0),
        smart_wallets,
    })
}

/// Which kind of subject this is, from the store.
async fn subject_kind(db: &SqlitePool, subject: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT kind FROM activity_daily WHERE subject = ? LIMIT 1")
        .bind(subject)
        .fetch_optional(db)
        .await
}

/// Normalises user input: contract ids and accounts upper-case, "xlm" ->
/// "XLM", "usdc:gA5Z..." -> "USDC:GA5Z...". Asset codes keep their case
/// otherwise (codes are case-sensitive on Stellar).
pub fn normalize_subject(input: &str) -> String {
    let s = input.trim();
    if s.eq_ignore_ascii_case("xlm") || s.eq_ignore_ascii_case("native") {
        return "XLM".to_string();
    }
    match s.split_once(':') {
        Some((code, issuer)) => format!("{}:{}", code.trim(), issuer.trim().to_ascii_uppercase()),
        None => s.to_ascii_uppercase(),
    }
}

pub async fn traction_report(
    store: &Analytics,
    stellar: &StellarService,
    subject_input: &str,
    days: i64,
    relayers: &[String],
) -> Result<Value, AppError> {
    let db = store.reader();
    let relayers: Vec<String> = relayers.iter().map(|r| r.trim().to_ascii_uppercase()).filter(|r| !r.is_empty()).collect();
    if let Some(bad) = relayers.iter().find(|r| !is_strkey(r)) {
        return Err(AppError::BadRequest(format!("'{bad}' is not a Stellar account or contract id")));
    }
    if relayers.len() > 20 {
        return Err(AppError::BadRequest("Declare at most 20 relayer accounts".into()));
    }
    let excl = exclusion(&relayers);
    let subject = normalize_subject(subject_input);
    let days = days.clamp(1, 90);

    let kind = subject_kind(db, &subject).await?.ok_or_else(|| {
        AppError::NotFound(format!(
            "No activity recorded for {subject} since BlocRA started collecting Stellar ledgers. \
             Use a Soroban contract id (C...) or an asset (XLM or CODE:ISSUER) that has been active."
        ))
    })?;

    // The period ends on the latest collected day (UTC), which may be partial.
    let (first_day, last_day): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT MIN(day), MAX(day) FROM collected_days").fetch_one(db).await?;
    let last_day = last_day.unwrap_or_else(|| day_of(now()));
    let first_day = first_day.unwrap_or_else(|| last_day.clone());
    let from = shift_day(&last_day, -(days - 1));
    let prev_to = shift_day(&from, -1);
    let prev_from = shift_day(&from, -days);
    let period = Period { from: &from, to: &last_day };
    let previous = Period { from: &prev_from, to: &prev_to };
    let has_previous = first_day.as_str() <= prev_to.as_str();

    let cur = totals(db, &subject, period, &excl).await?;
    let prev = if has_previous { Some(totals(db, &subject, previous, &excl).await?) } else { None };

    // Calls sent by the project's declared relayers that carried no user
    // authorization: their users can't be told apart, so they're reported
    // separately instead of as one hyperactive "user".
    let (relayer_calls, relayer_accounts): (Option<i64>, i64) = if relayers.is_empty() {
        (None, 0)
    } else {
        let list: Vec<String> = relayers.iter().map(|r| format!("'{r}'")).collect();
        sqlx::query_as(&format!(
            "SELECT SUM(calls), COUNT(DISTINCT account) FROM activity_daily
             WHERE subject = ? AND day BETWEEN ? AND ? AND account IN ({})",
            list.join(", ")
        ))
        .bind(&subject)
        .bind(period.from)
        .bind(period.to)
        .fetch_one(db)
        .await?
    };

    // New accounts: first seen (in all collected data) during the period.
    let new_accounts: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM (SELECT account, MIN(day) AS first FROM activity_daily WHERE subject = ?{excl} GROUP BY account)
         WHERE first BETWEEN ? AND ?",
    ))
    .bind(&subject)
    .bind(period.from)
    .bind(period.to)
    .fetch_one(db)
    .await?;
    let new_reliable = first_day.as_str() < period.from;

    // Returning: active in both periods.
    let returning: i64 = if has_previous {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM (
               SELECT account FROM activity_daily WHERE subject = ? AND day BETWEEN ? AND ?{excl}
               INTERSECT
               SELECT account FROM activity_daily WHERE subject = ? AND day BETWEEN ? AND ?{excl})",
        ))
        .bind(&subject)
        .bind(period.from)
        .bind(period.to)
        .bind(&subject)
        .bind(previous.from)
        .bind(previous.to)
        .fetch_one(db)
        .await?
    } else {
        0
    };

    // Daily series.
    let daily_rows: Vec<(String, i64, i64, i64, f64)> = sqlx::query_as(&format!(
        "SELECT day, COUNT(DISTINCT account), SUM(calls), SUM(failed), SUM(volume)
         FROM activity_daily WHERE subject = ? AND day BETWEEN ? AND ?{excl} GROUP BY day ORDER BY day",
    ))
    .bind(&subject)
    .bind(period.from)
    .bind(period.to)
    .fetch_all(db)
    .await?;
    let daily: Vec<Value> = daily_rows
        .iter()
        .map(|(day, a, c, f, v)| json!({ "day": day, "active_accounts": a, "calls": c, "failed": f, "volume": v }))
        .collect();

    // Concentration: who generates the activity.
    let per_account: Vec<(String, i64, i64)> = sqlx::query_as(&format!(
        "SELECT account, SUM(calls) AS c, MAX(calls) AS peak FROM activity_daily
         WHERE subject = ? AND day BETWEEN ? AND ?{excl} GROUP BY account ORDER BY c DESC",
    ))
    .bind(&subject)
    .bind(period.from)
    .bind(period.to)
    .fetch_all(db)
    .await?;
    let total_calls = cur.calls.max(1) as f64;
    let top10: i64 = per_account.iter().take(10).map(|a| a.1).sum();
    let bots: Vec<&(String, i64, i64)> = per_account.iter().filter(|a| a.2 >= BOT_CALLS_PER_DAY).collect();
    let bot_calls: i64 = bots.iter().map(|a| a.1).sum();
    let top_accounts: Vec<Value> = per_account
        .iter()
        .take(5)
        .map(|(acc, c, peak)| {
            json!({ "account": acc, "calls": c, "share_pct": (*c as f64 / total_calls * 10000.0).round() / 100.0,
                    "bot_like": *peak >= BOT_CALLS_PER_DAY })
        })
        .collect();
    let top10_share = (top10 as f64 / total_calls * 1000.0).round() / 10.0;
    let self_share = (cur.self_calls as f64 / total_calls * 1000.0).round() / 10.0;
    let relayed_share = (cur.relayed_calls as f64 / total_calls * 1000.0).round() / 10.0;
    let bot_share = (bot_calls as f64 / total_calls * 1000.0).round() / 10.0;

    // Functions (contracts only).
    let functions: Vec<Value> = if kind == "contract" {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT function, SUM(calls), SUM(failed) FROM function_daily
             WHERE contract = ? AND day BETWEEN ? AND ? GROUP BY function ORDER BY 2 DESC LIMIT 15",
        )
        .bind(&subject)
        .bind(period.from)
        .bind(period.to)
        .fetch_all(db)
        .await?;
        rows.into_iter()
            .map(|(f, c, x)| json!({ "function": f, "calls": c, "failed": x,
                                     "failed_pct": if c > 0 { (x as f64 / c as f64 * 1000.0).round() / 10.0 } else { 0.0 } }))
            .collect()
    } else {
        vec![]
    };

    // Ecosystem position among subjects of the same kind in the period.
    let peers: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT subject, COUNT(DISTINCT account) AS a, SUM(calls), SUM(failed) FROM activity_daily
         WHERE kind = ? AND day BETWEEN ? AND ? GROUP BY subject ORDER BY a DESC",
    )
    .bind(&kind)
    .bind(period.from)
    .bind(period.to)
    .fetch_all(db)
    .await?;
    let position = peers.iter().position(|p| p.0 == subject).map(|i| i + 1).unwrap_or(peers.len());
    let percentile = if peers.is_empty() {
        0.0
    } else {
        ((1.0 - (position as f64 - 1.0) / peers.len() as f64) * 1000.0).round() / 10.0
    };
    // Median failure rate of peers with real usage (>= 10 accounts).
    let mut peer_fail: Vec<f64> = peers
        .iter()
        .filter(|p| p.1 >= 10 && p.2 > 0)
        .map(|p| p.3 as f64 / p.2 as f64 * 100.0)
        .collect();
    peer_fail.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_fail = peer_fail.get(peer_fail.len() / 2).copied();

    // Coverage: ledgers in the period that went into the daily tables.
    type CoverageRow = (Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>);
    let (ledgers, l_from, l_to, t_from, t_to): CoverageRow =
        sqlx::query_as(
            "SELECT SUM(ledgers), MIN(first_ledger), MAX(last_ledger), MIN(first_ts), MAX(last_ts)
             FROM collected_days WHERE day BETWEEN ? AND ? AND first_ledger IS NOT NULL",
        )
        .bind(period.from)
        .bind(period.to)
        .fetch_one(db)
        .await?;
    let ledgers = ledgers.unwrap_or(0);
    let span_ledgers = match (l_from, l_to) {
        (Some(a), Some(b)) => b - a + 1,
        _ => 0,
    };
    let completeness = if span_ledgers > 0 { (ledgers as f64 / span_ledgers as f64 * 100.0).min(100.0) } else { 0.0 };
    let covered_days = match (t_from, t_to) {
        (Some(a), Some(b)) => (b - a) as f64 / 86_400.0,
        _ => 0.0,
    };
    let full_period = covered_days >= days as f64 - 1.0 && completeness >= 99.0;

    let failed_pct = if cur.calls > 0 { cur.failed as f64 / cur.calls as f64 * 100.0 } else { 0.0 };
    let prev_failed_pct = prev.as_ref().map(|p| if p.calls > 0 { p.failed as f64 / p.calls as f64 * 100.0 } else { 0.0 });

    let kpis = json!({
        "active_accounts": kpi(cur.active as f64, prev.as_ref().map(|p| p.active as f64)),
        "new_accounts": json!({ "value": new_accounts, "reliable": new_reliable }),
        "calls": kpi(cur.calls as f64, prev.as_ref().map(|p| p.calls as f64)),
        "failed_pct": kpi((failed_pct * 10.0).round() / 10.0, prev_failed_pct.map(|f| (f * 10.0).round() / 10.0)),
        "volume": if kind == "asset" { kpi(cur.volume, prev.as_ref().map(|p| p.volume)) } else { Value::Null },
        "calls_per_account": if cur.active > 0 { json!(((cur.calls as f64 / cur.active as f64) * 10.0).round() / 10.0) } else { json!(0) },
    });

    // Rule-based findings over the measured figures.
    let unit = if kind == "contract" { "calls" } else { "payments" };
    let mut findings: Vec<String> = Vec::new();
    if let (Some(p), Value::Number(ch)) = (&prev, &kpis["active_accounts"]["change_pct"]) {
        let ch = ch.as_f64().unwrap_or(0.0);
        findings.push(format!(
            "Active accounts {} {:.1}% vs the previous {days} days ({} → {}).",
            if ch >= 0.0 { "rose" } else { "fell" },
            ch.abs(),
            p.active,
            cur.active
        ));
    }
    if top10_share >= 50.0 && cur.active > 10 {
        findings.push(format!(
            "Activity is concentrated: the top 10 of {} accounts made {top10_share}% of {unit}.",
            cur.active
        ));
    }
    if !bots.is_empty() {
        findings.push(format!(
            "{} account{} made ≥{BOT_CALLS_PER_DAY} {unit} on a single day (automated); together {bot_share}% of all {unit}.",
            bots.len(),
            if bots.len() == 1 { "" } else { "s" }
        ));
    }
    if kind == "contract" && relayed_share >= 5.0 {
        findings.push(format!(
            "{relayed_share}% of calls were submitted by a relayer or sponsor on the user's behalf; each is counted for the account that authorized it, not the relayer."
        ));
    }
    if kind == "contract" && cur.smart_wallets > 0 {
        findings.push(format!(
            "{} of {} active accounts are smart wallets (contract accounts).",
            cur.smart_wallets, cur.active
        ));
    }
    if let Some(c) = relayer_calls.filter(|c| *c > 0) {
        findings.push(format!(
            "{c} calls came from the project's declared relayers without user authorization; their users can't be identified on-chain and are excluded from account counts."
        ));
    }
    if kind == "asset" && self_share >= 10.0 {
        findings.push(format!(
            "{self_share}% of payments were sent to the sender's own account (arbitrage loops); they are excluded from volume."
        ));
    }
    if let Some(m) = median_fail {
        let rel = if failed_pct > m * 1.5 && failed_pct - m > 2.0 {
            "well above"
        } else if failed_pct < m * 0.67 {
            "below"
        } else {
            "in line with"
        };
        findings.push(format!(
            "{failed_pct:.1}% of {unit} failed, {rel} the {m:.1}% median of comparable {}s.",
            if kind == "contract" { "contract" } else { "asset" }
        ));
    }
    if !peers.is_empty() {
        findings.push(format!(
            "Ranked #{position} of {} {}s by active accounts in this period (top {}%).",
            peers.len(),
            if kind == "contract" { "contract" } else { "asset" },
            // Share of peers at or above this rank, rounded up so #1 of 256 reads "top 1%".
            ((position as f64 / peers.len() as f64) * 100.0).ceil().max(1.0) as i64
        ));
    }
    if has_previous && prev.as_ref().is_some_and(|p| p.active > 0) {
        let p = prev.as_ref().map(|p| p.active).unwrap_or(1);
        findings.push(format!(
            "{returning} of the previous period's {p} accounts came back ({:.0}% retention).",
            returning as f64 / p as f64 * 100.0
        ));
    }
    if !full_period {
        let held = if covered_days < 1.0 {
            format!("{:.1} hours", covered_days * 24.0)
        } else {
            format!("{covered_days:.1} days")
        };
        findings.push(format!(
            "Partial data: BlocRA holds {held} of the {days}-day period ({completeness:.1}% of ledgers in that span). Figures cover only the collected time."
        ));
    }

    // Live asset facts from Horizon (holders and supply right now).
    let asset_snapshot = if kind == "asset" {
        stellar.asset_snapshot(&subject).await.ok().flatten()
    } else {
        None
    };

    Ok(json!({
        "version": 1,
        "type": "traction",
        "method_version": crate::services::attestation::METHOD_VERSION,
        "chain": "stellar",
        "subject": { "id": subject, "kind": kind, "label": label(&subject, &kind) },
        "network": stellar.network,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "period": { "days": days, "from": period.from, "to": period.to,
                    "previous_from": previous.from, "previous_to": previous.to, "has_previous": has_previous },
        "kpis": kpis,
        "daily": daily,
        "retention": { "returning_accounts": returning,
                       "previous_active": prev.as_ref().map(|p| p.active),
                       "returning_pct": prev.as_ref().filter(|p| p.active > 0).map(|p| (returning as f64 / p.active as f64 * 1000.0).round() / 10.0) },
        "quality": { "top10_share_pct": top10_share, "bot_like_accounts": bots.len(), "bot_like_share_pct": bot_share,
                     "bot_threshold_calls_per_day": BOT_CALLS_PER_DAY, "top_accounts": top_accounts,
                     "self_payments": if kind == "asset" { json!(cur.self_calls) } else { Value::Null },
                     "self_payment_share_pct": if kind == "asset" { json!(self_share) } else { Value::Null },
                     "relayed_calls": if kind == "contract" { json!(cur.relayed_calls) } else { Value::Null },
                     "relayed_share_pct": if kind == "contract" { json!(relayed_share) } else { Value::Null },
                     "smart_wallet_accounts": if kind == "contract" { json!(cur.smart_wallets) } else { Value::Null } },
        "declared_relayers": { "accounts": relayers, "active": relayer_accounts, "calls_without_user_auth": relayer_calls },
        "functions": functions,
        "ecosystem": { "rank": position, "of": peers.len(), "percentile": percentile,
                       "median_failed_pct": median_fail.map(|m| (m * 10.0).round() / 10.0), "metric": "active_accounts" },
        "asset_snapshot": asset_snapshot,
        "findings": findings,
        "coverage": {
            "collection_started": first_day,
            "ledgers_collected": ledgers,
            "ledger_from": l_from, "ledger_to": l_to,
            "completeness_pct": (completeness * 10.0).round() / 10.0,
            "days_covered": (covered_days * 100.0).round() / 100.0,
            "hours_covered": (covered_days * 240.0).round() / 10.0,
            "expected_ledgers": (LEDGERS_PER_DAY * days as f64).round(),
            "full_period": full_period
        },
        "methodology": methodology(&kind),
    }))
}

fn label(subject: &str, kind: &str) -> String {
    match (kind, subject.split_once(':')) {
        ("asset", Some((code, issuer))) => format!("{code} (issuer {}…{})", &issuer[..4.min(issuer.len())], &issuer[issuer.len().saturating_sub(4)..]),
        ("asset", None) => "XLM (native)".to_string(),
        _ => format!("Contract {}…{}", &subject[..6.min(subject.len())], &subject[subject.len().saturating_sub(4)..]),
    }
}

fn methodology(kind: &str) -> Vec<String> {
    let mut m = vec![
        "Source: every Stellar ledger read from Horizon by BlocRA's collector and stored with its transactions and operations.".to_string(),
        "Days are UTC calendar days; the latest day may be partial.".to_string(),
    ];
    if kind == "contract" {
        m.push("A call is one invoke_host_function operation targeting the contract. Its account is the first account (G...) or smart wallet (C...) that signed an authorization entry for it, so calls sent through relayers, fee sponsors or fee bumps count for the user; calls without authorization entries count for the operation's source. Failed calls are included in calls and counted in failed.".to_string());
        m.push("Declared relayers (accounts the project names as its own relayers) are left out of account counts, concentration and retention; calls they sent without user authorization are reported separately.".to_string());
    } else {
        m.push("Activity is payments and path payments of the asset; the account is the sender. Volume sums amounts of successful payments to other accounts, in the asset's units; payments to oneself (arbitrage loops) are counted as self-payments and left out of volume.".to_string());
    }
    m.push(format!(
        "Automated (bot-like) accounts made at least {BOT_CALLS_PER_DAY} {} on a single day.",
        if kind == "contract" { "calls" } else { "payments" }
    ));
    m.push("New accounts were first seen during the period in all data BlocRA has collected; with less history than the period, they are marked unreliable.".to_string());
    m.push("Ecosystem rank compares active accounts with every contract (or asset) active in the same period; the failure-rate median uses peers with at least 10 active accounts.".to_string());
    m
}

/// Most active subjects of a kind over the last `days` days, for pickers.
pub async fn top_subjects(store: &Analytics, kind: &str, days: i64, limit: i64) -> Result<Vec<Value>, AppError> {
    let db = store.reader();
    let last: Option<String> = sqlx::query_scalar("SELECT MAX(day) FROM collected_days").fetch_one(db).await?;
    let last = last.unwrap_or_else(|| day_of(now()));
    let from = shift_day(&last, -(days.clamp(1, 90) - 1));
    let rows: Vec<(String, i64, i64, f64)> = sqlx::query_as(
        "SELECT subject, COUNT(DISTINCT account) AS a, SUM(calls), SUM(volume) FROM activity_daily
         WHERE kind = ? AND day BETWEEN ? AND ? GROUP BY subject ORDER BY a DESC LIMIT ?",
    )
    .bind(kind)
    .bind(&from)
    .bind(&last)
    .bind(limit.clamp(1, 100))
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(s, a, c, v)| json!({ "id": s, "label": label(&s, kind), "kind": kind, "active_accounts": a, "calls": c, "volume": v }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_subjects() {
        assert_eq!(normalize_subject(" xlm "), "XLM");
        assert_eq!(normalize_subject("USDC:ga5zsej"), "USDC:GA5ZSEJ");
        assert_eq!(normalize_subject("cabc"), "CABC");
        assert_eq!(shift_day("2026-11-06", -6), "2026-10-31");
        assert_eq!(change_pct(120.0, 100.0), json!(20.0));
        assert_eq!(change_pct(1.0, 0.0), Value::Null);
    }

    #[test]
    fn validates_declared_relayers() {
        let g = "GBQ6QIHVDHFSY4CDORDELIZXRUWF72FTCRNOXUFPH7YOO3PFFMXLAVUN";
        assert!(is_strkey(g));
        assert!(is_strkey("CDL74RF5BLYR2YBLCCI7F5FB6TPSCLKEJUBSD2RSVWZ4YHF3VMFAIGWA"));
        assert!(!is_strkey("G' OR 1=1 --"));
        assert!(!is_strkey(&g.to_lowercase()));
        assert!(!is_strkey("SBQ6QIHVDHFSY4CDORDELIZXRUWF72FTCRNOXUFPH7YOO3PFFMXLAVUN"));
        assert_eq!(exclusion(&[]), "");
        assert_eq!(exclusion(&[g.to_string()]), format!(" AND account NOT IN ('{g}')"));
    }
}
