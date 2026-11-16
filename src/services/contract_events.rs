//! Contract events from Stellar RPC `getEvents`, decoded.
//!
//! Every event a contract emits is kept with its name (the first topic, when
//! it is a symbol or string) and its topics and data as JSON. Token events
//! are also read as movements of value:
//!
//! - `transfer`: `[transfer, from, to, (asset)]`, data `amount` or
//!   `{amount, to_muxed_id}` (CAP-67).
//! - `mint`: `[mint, to, asset]` (CAP-67) or `[mint, admin, to, (asset)]`
//!   (SEP-41 and older Stellar Asset Contracts).
//! - `burn`: `[burn, from, (asset)]`.
//! - `clawback`: `[clawback, from, asset]` (CAP-67) or
//!   `[clawback, admin, from, (asset)]`.
//!
//! The trailing asset topic (a SEP-11 string such as `native` or
//! `USDC:G...`) only appears on Stellar Asset Contracts. `fee` events (the
//! fee each transaction pays in XLM) are skipped: Horizon already records
//! fees, and they would dominate storage.

use serde::Deserialize;
use serde_json::Value;
use stellar_xdr::ScVal;

use crate::services::xdr_json::{address, amount, decode, text, to_json};

/// One event as `getEvents` returns it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub ledger: u64,
    #[serde(default)]
    pub contract_id: String,
    pub id: String,
    #[serde(default)]
    pub operation_index: u32,
    pub tx_hash: String,
    #[serde(default)]
    pub topic: Vec<String>,
    #[serde(default)]
    pub value: String,
}

/// A value moved by a token event.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenMove {
    /// transfer, mint, burn or clawback.
    pub kind: &'static str,
    pub from: Option<String>,
    pub to: Option<String>,
    /// In the token's smallest unit.
    pub amount: i128,
    /// SEP-11 asset of a Stellar Asset Contract (`native`, `CODE:ISSUER`).
    pub asset: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DecodedEvent {
    pub id: String,
    pub tx_hash: String,
    pub op_index: u32,
    pub contract: String,
    /// First topic, when it is a symbol or string.
    pub name: Option<String>,
    pub topics: Vec<Value>,
    pub data: Value,
    pub token_move: Option<TokenMove>,
}

/// Decodes a contract event. `None` for system and diagnostic events, `fee`
/// events, and events whose XDR can't be read.
pub fn decode_event(e: &RpcEvent) -> Option<DecodedEvent> {
    if e.event_type != "contract" || e.contract_id.is_empty() {
        return None;
    }
    let topics: Vec<ScVal> = e.topic.iter().map(|t| decode(t)).collect::<Option<_>>()?;
    let data = if e.value.is_empty() { ScVal::Void } else { decode(&e.value)? };
    let name = topics.first().and_then(text);
    if name.as_deref() == Some("fee") {
        return None;
    }
    Some(DecodedEvent {
        id: e.id.clone(),
        tx_hash: e.tx_hash.clone(),
        op_index: e.operation_index,
        contract: e.contract_id.clone(),
        token_move: name.as_deref().and_then(|n| token_move(n, &topics, &data)),
        name,
        topics: topics.iter().map(to_json).collect(),
        data: to_json(&data),
    })
}

/// Amount of a token event's data: an integer, or `{amount, ...}`.
fn data_amount(data: &ScVal) -> Option<i128> {
    match data {
        ScVal::Map(Some(m)) => m.0.iter().find(|e| text(&e.key).as_deref() == Some("amount")).and_then(|e| amount(&e.val)),
        other => amount(other),
    }
}

pub fn token_move(name: &str, topics: &[ScVal], data: &ScVal) -> Option<TokenMove> {
    let kind: &'static str = match name {
        "transfer" => "transfer",
        "mint" => "mint",
        "burn" => "burn",
        "clawback" => "clawback",
        _ => return None,
    };
    let amount = data_amount(data)?;
    // The trailing SEP-11 asset string of Stellar Asset Contracts.
    let (asset, rest) = match topics.split_last() {
        Some((last @ ScVal::String(_), rest)) if topics.len() > 1 => (text(last), rest),
        _ => (None, topics),
    };
    let addresses: Vec<String> = rest.iter().skip(1).map(address).collect::<Option<_>>()?;
    let (from, to) = match (kind, addresses.as_slice()) {
        ("transfer", [from, to]) => (Some(from.clone()), Some(to.clone())),
        // [mint, to] (CAP-67) or [mint, admin, to] (SEP-41): the recipient is last.
        ("mint", [.., to]) if addresses.len() <= 2 => (None, Some(to.clone())),
        ("burn", [from]) => (Some(from.clone()), None),
        // [clawback, from] (CAP-67) or [clawback, admin, from]: the holder is last.
        ("clawback", [.., from]) if addresses.len() <= 2 => (Some(from.clone()), None),
        _ => return None,
    };
    Some(TokenMove { kind, from, to, amount, asset })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(topic: &[&str], value: &str) -> RpcEvent {
        RpcEvent {
            event_type: "contract".into(),
            ledger: 65511859,
            contract_id: "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA".into(),
            id: "0281371291905208320-0000000000".into(),
            operation_index: 0,
            tx_hash: "50e2aab25b959ec2bf0793e6db4fa551f732638ba3935bfef3060cfb769dca8a".into(),
            topic: topic.iter().map(|s| s.to_string()).collect(),
            value: value.into(),
        }
    }

    const G1: &str = "AAAAEgAAAAAAAAAArM6E+6Qbpk3s1IjTG0SqPVBvupQaHf1Bmc0suQbiU+8=";
    const G2: &str = "AAAAEgAAAAAAAAAA95bqfoQYpYDKUyxz44VEwNqkM4kIkkTzsaxdGkF7pHM=";
    const NATIVE: &str = "AAAADgAAAAZuYXRpdmUAAA==";
    const AMOUNT_100: &str = "AAAACgAAAAAAAAAAAAAAAAAAAGQ=";

    #[test]
    fn reads_a_cap67_transfer_from_mainnet() {
        // Recorded from mainnet ledger 65511859.
        let e = ev(
            &["AAAADwAAAAh0cmFuc2Zlcg==", G1, G2, NATIVE],
            "AAAAEQAAAAEAAAACAAAADwAAAAZhbW91bnQAAAAAAAoAAAAAAAAAAAAAAAAAG7uuAAAADwAAAAt0b19tdXhlZF9pZAAAAAAFAAAAAAAANYY=",
        );
        let d = decode_event(&e).unwrap();
        assert_eq!(d.name.as_deref(), Some("transfer"));
        let m = d.token_move.unwrap();
        assert_eq!((m.kind, m.amount, m.asset.as_deref()), ("transfer", 1817518, Some("native")));
        assert!(m.from.unwrap().starts_with('G') && m.to.unwrap().starts_with('G'));
        assert_eq!(d.topics.len(), 4);
        assert_eq!(d.data["amount"], "1817518");
    }

    #[test]
    fn reads_mint_and_clawback_in_both_layouts() {
        let mint = |topics: &[&str]| token_move("mint", &topics.iter().map(|t| decode(t).unwrap()).collect::<Vec<_>>(), &decode(AMOUNT_100).unwrap());
        // CAP-67: [mint, to, asset]
        let m = mint(&["AAAADwAAAARtaW50", G2, NATIVE]).unwrap();
        assert_eq!((m.from, m.to.is_some(), m.amount), (None, true, 100));
        // SEP-41: [mint, admin, to]
        let m = mint(&["AAAADwAAAARtaW50", G1, G2]).unwrap();
        assert_eq!(m.to, address(&decode(G2).unwrap()));
        let claw = token_move("clawback", &[decode("AAAADwAAAAhjbGF3YmFjaw==").unwrap(), decode(G1).unwrap(), decode(G2).unwrap(), decode(NATIVE).unwrap()], &decode(AMOUNT_100).unwrap()).unwrap();
        assert_eq!(claw.from, address(&decode(G2).unwrap()));
    }

    #[test]
    fn skips_fee_events_and_keeps_other_events() {
        assert!(decode_event(&ev(&["AAAADwAAAANmZWUA", G1], AMOUNT_100)).is_none());
        // A non-token event keeps its name and decoded topics.
        let d = decode_event(&ev(&["AAAADwAAAAlzZXRfcHJpY2UAAAA=", G1], AMOUNT_100)).unwrap();
        assert_eq!(d.name.as_deref(), Some("set_price"));
        assert!(d.token_move.is_none());
        assert_eq!(d.data, Value::String("100".into()));
    }

    #[test]
    fn rejects_malformed_token_events() {
        // A transfer without a recipient is not a movement.
        assert!(token_move("transfer", &[decode("AAAADwAAAAh0cmFuc2Zlcg==").unwrap(), decode(G1).unwrap()], &decode(AMOUNT_100).unwrap()).is_none());
        // No amount.
        assert!(token_move("burn", &[decode("AAAADwAAAARidXJu").unwrap(), decode(G1).unwrap()], &ScVal::Void).is_none());
    }
}
