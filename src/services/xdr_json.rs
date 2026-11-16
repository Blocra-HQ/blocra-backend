//! Soroban values (ScVal) as plain JSON, for storing and querying contract
//! events and call arguments.
//!
//! - Addresses become strkeys (G..., C..., M..., L..., B...).
//! - 32- and 64-bit integers become JSON numbers; 128- and 256-bit integers
//!   become decimal strings (128) or 0x-hex (256) so nothing is rounded.
//! - Bytes become lowercase hex, symbols and strings become text.
//! - Maps whose keys are all symbols or strings become objects; other maps
//!   become a list of `[key, value]` pairs.

use serde_json::{json, Map, Value};
use stellar_xdr::{Int128Parts, Limits, ReadXdr, ScVal, UInt128Parts};

pub fn i128_of(p: &Int128Parts) -> i128 {
    ((p.hi as i128) << 64) | p.lo as i128
}

pub fn u128_of(p: &UInt128Parts) -> u128 {
    ((p.hi as u128) << 64) | p.lo as u128
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Text of a symbol or string value.
pub fn text(v: &ScVal) -> Option<String> {
    match v {
        ScVal::Symbol(s) => Some(s.0.to_utf8_string_lossy()),
        ScVal::String(s) => Some(s.0.to_utf8_string_lossy()),
        _ => None,
    }
}

/// Strkey of an address value.
pub fn address(v: &ScVal) -> Option<String> {
    match v {
        ScVal::Address(a) => Some(a.to_string()),
        _ => None,
    }
}

/// A signed amount (i128, or any smaller integer) as i128.
pub fn amount(v: &ScVal) -> Option<i128> {
    match v {
        ScVal::I128(p) => Some(i128_of(p)),
        ScVal::U128(p) => i128::try_from(u128_of(p)).ok(),
        ScVal::I64(n) => Some(*n as i128),
        ScVal::U64(n) => Some(*n as i128),
        ScVal::I32(n) => Some(*n as i128),
        ScVal::U32(n) => Some(*n as i128),
        _ => None,
    }
}

pub fn to_json(v: &ScVal) -> Value {
    match v {
        ScVal::Bool(b) => json!(b),
        ScVal::Void => Value::Null,
        ScVal::Error(e) => json!({ "error": format!("{e:?}") }),
        ScVal::U32(n) => json!(n),
        ScVal::I32(n) => json!(n),
        ScVal::U64(n) => json!(n),
        ScVal::I64(n) => json!(n),
        ScVal::Timepoint(t) => json!(t.0),
        ScVal::Duration(d) => json!(d.0),
        ScVal::U128(p) => json!(u128_of(p).to_string()),
        ScVal::I128(p) => json!(i128_of(p).to_string()),
        ScVal::U256(p) => {
            let mut b = Vec::with_capacity(32);
            for part in [p.hi_hi, p.hi_lo, p.lo_hi, p.lo_lo] {
                b.extend_from_slice(&part.to_be_bytes());
            }
            json!(format!("0x{}", hex(&b)))
        }
        ScVal::I256(p) => {
            let mut b = Vec::with_capacity(32);
            b.extend_from_slice(&p.hi_hi.to_be_bytes());
            for part in [p.hi_lo, p.lo_hi, p.lo_lo] {
                b.extend_from_slice(&part.to_be_bytes());
            }
            json!(format!("0x{}", hex(&b)))
        }
        ScVal::Bytes(b) => json!(hex(b.0.as_slice())),
        ScVal::String(s) => json!(s.0.to_utf8_string_lossy()),
        ScVal::Symbol(s) => json!(s.0.to_utf8_string_lossy()),
        ScVal::Vec(items) => Value::Array(items.as_ref().map(|v| v.iter().map(to_json).collect()).unwrap_or_default()),
        ScVal::Map(entries) => {
            let entries = entries.as_ref().map(|m| m.0.as_slice()).unwrap_or_default();
            if entries.iter().all(|e| text(&e.key).is_some()) {
                let mut obj = Map::new();
                for e in entries {
                    obj.insert(text(&e.key).unwrap_or_default(), to_json(&e.val));
                }
                Value::Object(obj)
            } else {
                Value::Array(entries.iter().map(|e| json!([to_json(&e.key), to_json(&e.val)])).collect())
            }
        }
        ScVal::Address(a) => json!(a.to_string()),
        ScVal::ContractInstance(_) => json!("(contract instance)"),
        ScVal::LedgerKeyContractInstance => json!("(contract instance key)"),
        ScVal::LedgerKeyNonce(n) => json!({ "nonce": n.nonce }),
        ScVal::ExecutableTag(s) => json!(s.0.to_utf8_string_lossy()),
    }
}

/// Decodes a base64 ScVal.
pub fn decode(b64: &str) -> Option<ScVal> {
    ScVal::from_xdr_base64(b64, Limits::depth(64)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_values_from_mainnet_events() {
        // A CAP-67 transfer's data: { amount: i128, to_muxed_id: u64 }.
        let v = decode("AAAAEQAAAAEAAAACAAAADwAAAAZhbW91bnQAAAAAAAoAAAAAAAAAAAAAAAAAG7uuAAAADwAAAAt0b19tdXhlZF9pZAAAAAAFAAAAAAAANYY=").unwrap();
        assert_eq!(to_json(&v), json!({ "amount": "1817518", "to_muxed_id": 13702 }));
        let addr = decode("AAAAEgAAAAAAAAAAYqaZTF7oxDF1XjY4sk23O7q8oIDv6mJGbzMkVFKqu7Q=").unwrap();
        let s = address(&addr).unwrap();
        assert!(s.starts_with('G') && s.len() == 56, "{s}");
        assert_eq!(text(&decode("AAAADwAAAANmZWUA").unwrap()).as_deref(), Some("fee"));
        assert_eq!(text(&decode("AAAADgAAAAZuYXRpdmUAAA==").unwrap()).as_deref(), Some("native"));
        assert_eq!(amount(&decode("AAAACgAAAAAAAAAAAAAAAAAAAGQ=").unwrap()), Some(100));
    }

    #[test]
    fn keeps_big_integers_exact() {
        let v = ScVal::I128(Int128Parts { hi: -1, lo: u64::MAX - 4 });
        assert_eq!(to_json(&v), json!("-5"));
        assert_eq!(amount(&v), Some(-5));
        let big = ScVal::U128(UInt128Parts { hi: 1, lo: 0 });
        assert_eq!(to_json(&big), json!("18446744073709551616"));
    }
}
