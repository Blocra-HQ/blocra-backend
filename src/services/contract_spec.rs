//! A contract's interface, read from the chain.
//!
//! Every Soroban contract built with the Rust SDK embeds its interface in a
//! WASM custom section, `contractspecv0`: its functions with argument names
//! and types, its own types, and (SDK 23+) the events it emits. BlocRA reads
//! the contract's instance (which code it runs), then the code, then that
//! section, so calls and events can be shown with their real field names.
//!
//! Stellar Asset Contracts (the built-in token contract of a classic asset)
//! have no WASM; their interface is the standard token interface, and their
//! name and symbol come from the instance storage.

use serde_json::{json, Map, Value};
use std::io::Cursor;
use stellar_xdr::{
    ContractDataDurability, ContractExecutable, ContractId, Hash, LedgerEntryData, LedgerKey, LedgerKeyContractCode,
    LedgerKeyContractData, Limited, Limits, ReadXdr, ScAddress, ScMetaEntry, ScSpecEntry, ScSpecEventDataFormat,
    ScSpecEventParamLocationV0, ScSpecTypeDef, ScSpecUdtUnionCaseV0, ScVal, WriteXdr,
};

use crate::errors::AppError;
use crate::services::soroban_rpc::SorobanRpc;
use crate::services::xdr_json::{text, to_json};

/// The payloads of a WASM module's custom sections with this name.
pub fn custom_sections<'a>(wasm: &'a [u8], wanted: &str) -> Vec<&'a [u8]> {
    fn leb(b: &[u8], at: &mut usize) -> Option<usize> {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let byte = *b.get(*at)?;
            *at += 1;
            value |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
            if shift > 28 {
                return None;
            }
        }
    }
    let mut out = Vec::new();
    if wasm.get(0..4) != Some(b"\0asm") {
        return out;
    }
    let mut at = 8;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let Some(size) = leb(wasm, &mut at) else { break };
        let end = at.saturating_add(size).min(wasm.len());
        if id == 0 {
            let mut p = at;
            if let Some(len) = leb(wasm, &mut p) {
                if wasm.get(p..p + len) == Some(wanted.as_bytes()) {
                    out.push(&wasm[p + len..end]);
                }
            }
        }
        at = end;
    }
    out
}

/// The interface entries in a contract's WASM.
pub fn spec_entries(wasm: &[u8]) -> Vec<ScSpecEntry> {
    let mut entries = Vec::new();
    for section in custom_sections(wasm, "contractspecv0") {
        let mut r = Limited::new(Cursor::new(section), Limits::depth(64));
        for e in ScSpecEntry::read_xdr_iter(&mut r) {
            match e {
                Ok(e) => entries.push(e),
                Err(_) => break,
            }
        }
    }
    entries
}

/// Build metadata (Rust and SDK versions, and anything the author added).
fn meta(wasm: &[u8]) -> Map<String, Value> {
    let mut out = Map::new();
    for section in custom_sections(wasm, "contractmetav0") {
        let mut r = Limited::new(Cursor::new(section), Limits::depth(16));
        for e in ScMetaEntry::read_xdr_iter(&mut r) {
            let Ok(ScMetaEntry::ScMetaV0(m)) = e else { break };
            out.insert(m.key.to_utf8_string_lossy(), json!(m.val.to_utf8_string_lossy()));
        }
    }
    out
}

/// A spec type as text, e.g. `Vec<Address>`, `Option<i128>`, `BytesN<32>`.
pub fn type_name(t: &ScSpecTypeDef) -> String {
    match t {
        ScSpecTypeDef::Val => "Val".into(),
        ScSpecTypeDef::Bool => "bool".into(),
        ScSpecTypeDef::Void => "()".into(),
        ScSpecTypeDef::Error => "Error".into(),
        ScSpecTypeDef::U32 => "u32".into(),
        ScSpecTypeDef::I32 => "i32".into(),
        ScSpecTypeDef::U64 => "u64".into(),
        ScSpecTypeDef::I64 => "i64".into(),
        ScSpecTypeDef::Timepoint => "Timepoint".into(),
        ScSpecTypeDef::Duration => "Duration".into(),
        ScSpecTypeDef::U128 => "u128".into(),
        ScSpecTypeDef::I128 => "i128".into(),
        ScSpecTypeDef::U256 => "u256".into(),
        ScSpecTypeDef::I256 => "i256".into(),
        ScSpecTypeDef::Bytes => "Bytes".into(),
        ScSpecTypeDef::String => "String".into(),
        ScSpecTypeDef::Symbol => "Symbol".into(),
        ScSpecTypeDef::Address => "Address".into(),
        ScSpecTypeDef::MuxedAddress => "MuxedAddress".into(),
        ScSpecTypeDef::Option(o) => format!("Option<{}>", type_name(&o.value_type)),
        ScSpecTypeDef::Result(r) => format!("Result<{}, {}>", type_name(&r.ok_type), type_name(&r.error_type)),
        ScSpecTypeDef::Vec(v) => format!("Vec<{}>", type_name(&v.element_type)),
        ScSpecTypeDef::Map(m) => format!("Map<{}, {}>", type_name(&m.key_type), type_name(&m.value_type)),
        ScSpecTypeDef::Tuple(t) => format!("({})", t.value_types.iter().map(type_name).collect::<Vec<_>>().join(", ")),
        ScSpecTypeDef::BytesN(b) => format!("BytesN<{}>", b.n),
        ScSpecTypeDef::Udt(u) => u.name.to_utf8_string_lossy(),
    }
}

fn doc(s: &stellar_xdr::StringM<1024>) -> Value {
    let d = s.to_utf8_string_lossy();
    if d.is_empty() {
        Value::Null
    } else {
        json!(d)
    }
}

/// Functions, events and types of a spec, as JSON.
pub fn describe(entries: &[ScSpecEntry]) -> Value {
    let (mut functions, mut events, mut types) = (Vec::new(), Vec::new(), Vec::new());
    for e in entries {
        match e {
            ScSpecEntry::FunctionV0(f) => functions.push(json!({
                "name": f.name.0.to_utf8_string_lossy(),
                "doc": doc(&f.doc),
                "inputs": f.inputs.iter().map(|i| json!({ "name": i.name.to_utf8_string_lossy(), "type": type_name(&i.type_) })).collect::<Vec<_>>(),
                "outputs": f.outputs.iter().map(type_name).collect::<Vec<_>>(),
            })),
            ScSpecEntry::EventV0(ev) => events.push(json!({
                "name": ev.name.to_utf8_string_lossy(),
                "doc": doc(&ev.doc),
                "prefix_topics": ev.prefix_topics.iter().map(|t| t.0.to_utf8_string_lossy()).collect::<Vec<_>>(),
                "params": ev.params.iter().map(|p| json!({
                    "name": p.name.to_utf8_string_lossy(),
                    "type": type_name(&p.type_),
                    "location": match p.location { ScSpecEventParamLocationV0::TopicList => "topic", ScSpecEventParamLocationV0::Data => "data" },
                })).collect::<Vec<_>>(),
                "data_format": match ev.data_format {
                    ScSpecEventDataFormat::SingleValue => "single_value",
                    ScSpecEventDataFormat::Vec => "vec",
                    ScSpecEventDataFormat::Map => "map",
                },
            })),
            ScSpecEntry::UdtStructV0(s) => types.push(json!({
                "kind": "struct", "name": s.name.to_utf8_string_lossy(),
                "fields": s.fields.iter().map(|f| json!({ "name": f.name.to_utf8_string_lossy(), "type": type_name(&f.type_) })).collect::<Vec<_>>(),
            })),
            ScSpecEntry::UdtUnionV0(u) => types.push(json!({
                "kind": "union", "name": u.name.to_utf8_string_lossy(),
                "cases": u.cases.iter().map(|c| match c {
                    ScSpecUdtUnionCaseV0::VoidV0(v) => json!({ "name": v.name.to_utf8_string_lossy() }),
                    ScSpecUdtUnionCaseV0::TupleV0(t) => json!({ "name": t.name.to_utf8_string_lossy(), "types": t.type_.iter().map(type_name).collect::<Vec<_>>() }),
                }).collect::<Vec<_>>(),
            })),
            ScSpecEntry::UdtEnumV0(u) => types.push(json!({
                "kind": "enum", "name": u.name.to_utf8_string_lossy(),
                "cases": u.cases.iter().map(|c| json!({ "name": c.name.to_utf8_string_lossy(), "value": c.value })).collect::<Vec<_>>(),
            })),
            ScSpecEntry::UdtErrorEnumV0(u) => types.push(json!({
                "kind": "error", "name": u.name.to_utf8_string_lossy(),
                "cases": u.cases.iter().map(|c| json!({ "name": c.name.to_utf8_string_lossy(), "value": c.value })).collect::<Vec<_>>(),
            })),
        }
    }
    json!({ "functions": functions, "events": events, "types": types })
}

/// The standard token interface every Stellar Asset Contract implements.
fn stellar_asset_interface() -> Value {
    let f = |name: &str, inputs: &[(&str, &str)], output: Option<&str>| {
        json!({
            "name": name, "doc": Value::Null,
            "inputs": inputs.iter().map(|(n, t)| json!({ "name": n, "type": t })).collect::<Vec<_>>(),
            "outputs": output.into_iter().collect::<Vec<_>>(),
        })
    };
    json!({
        "functions": [
            f("allowance", &[("from", "Address"), ("spender", "Address")], Some("i128")),
            f("approve", &[("from", "Address"), ("spender", "Address"), ("amount", "i128"), ("expiration_ledger", "u32")], None),
            f("balance", &[("id", "Address")], Some("i128")),
            f("transfer", &[("from", "Address"), ("to", "MuxedAddress"), ("amount", "i128")], None),
            f("transfer_from", &[("spender", "Address"), ("from", "Address"), ("to", "Address"), ("amount", "i128")], None),
            f("burn", &[("from", "Address"), ("amount", "i128")], None),
            f("burn_from", &[("spender", "Address"), ("from", "Address"), ("amount", "i128")], None),
            f("decimals", &[], Some("u32")),
            f("name", &[], Some("String")),
            f("symbol", &[], Some("String")),
            f("admin", &[], Some("Address")),
            f("set_admin", &[("new_admin", "Address")], None),
            f("authorized", &[("id", "Address")], Some("bool")),
            f("set_authorized", &[("id", "Address"), ("authorize", "bool")], None),
            f("mint", &[("to", "Address"), ("amount", "i128")], None),
            f("clawback", &[("from", "Address"), ("amount", "i128")], None),
        ],
        "events": [],
        "types": [],
    })
}

fn contract_id(id: &str) -> Result<[u8; 32], AppError> {
    stellar_strkey::Contract::from_string(id)
        .map(|c| c.0)
        .map_err(|_| AppError::BadRequest(format!("Not a contract id: {id}")))
}

fn key_b64(key: &LedgerKey) -> String {
    key.to_xdr_base64(Limits::none()).unwrap_or_default()
}

/// Reads a contract's interface from the chain.
pub async fn fetch(rpc: &SorobanRpc, id: &str) -> Result<Value, AppError> {
    let instance_key = key_b64(&LedgerKey::ContractData(LedgerKeyContractData {
        contract: ScAddress::Contract(ContractId(Hash(contract_id(id)?))),
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    }));
    let entry = rpc
        .ledger_entries(&[instance_key])
        .await?
        .pop()
        .flatten()
        .ok_or_else(|| AppError::NotFound(format!("No contract {id} on Stellar mainnet")))?;
    let Ok(LedgerEntryData::ContractData(data)) = LedgerEntryData::from_xdr_base64(&entry, Limits::depth(64)) else {
        return Err(AppError::BadRequest("Unexpected contract instance entry".into()));
    };
    let ScVal::ContractInstance(instance) = &data.val else {
        return Err(AppError::BadRequest("Unexpected contract instance value".into()));
    };
    match &instance.executable {
        ContractExecutable::StellarAsset => {
            // Instance storage holds METADATA = { decimal, name, symbol }.
            let metadata = instance.storage.as_ref().and_then(|m| {
                m.0.iter().find(|e| text(&e.key).as_deref() == Some("METADATA")).map(|e| to_json(&e.val))
            });
            let mut out = stellar_asset_interface();
            out["contract"] = json!(id);
            out["kind"] = json!("stellar_asset");
            out["wasm_hash"] = Value::Null;
            out["asset"] = metadata.unwrap_or(Value::Null);
            out["meta"] = json!({});
            Ok(out)
        }
        ContractExecutable::Wasm(hash) => {
            let code_key = key_b64(&LedgerKey::ContractCode(LedgerKeyContractCode { hash: hash.clone() }));
            let code = rpc
                .ledger_entries(&[code_key])
                .await?
                .pop()
                .flatten()
                .ok_or_else(|| AppError::NotFound(format!("The code of {id} is not on the ledger (archived?)")))?;
            let Ok(LedgerEntryData::ContractCode(code)) = LedgerEntryData::from_xdr_base64(&code, Limits::none()) else {
                return Err(AppError::BadRequest("Unexpected contract code entry".into()));
            };
            let wasm = code.code.as_slice();
            let mut out = describe(&spec_entries(wasm));
            out["contract"] = json!(id);
            out["kind"] = json!("wasm");
            out["wasm_hash"] = json!(hash.0.iter().map(|b| format!("{b:02x}")).collect::<String>());
            out["asset"] = Value::Null;
            out["meta"] = Value::Object(meta(wasm));
            Ok(out)
        }
        other => {
            let mut out = json!({ "functions": [], "events": [], "types": [], "meta": {}, "asset": null, "wasm_hash": null });
            out["contract"] = json!(id);
            out["kind"] = json!(format!("{other:?}").to_lowercase());
            Ok(out)
        }
    }
}

/// Names a call's arguments from the interface: `{arg_name: value}`, or the
/// arguments as a list when the function isn't in the interface.
pub fn name_call(interface: &Value, function: &str, args: &[Value]) -> Value {
    let inputs = interface["functions"]
        .as_array()
        .and_then(|fs| fs.iter().find(|f| f["name"] == function))
        .and_then(|f| f["inputs"].as_array());
    match inputs {
        Some(inputs) if inputs.len() == args.len() => Value::Object(
            inputs.iter().zip(args).map(|(i, a)| (i["name"].as_str().unwrap_or("?").to_string(), a.clone())).collect(),
        ),
        _ => Value::Array(args.to_vec()),
    }
}

/// Names an event from the interface: `(event name, {field: value})`, or
/// `None` when no declared event matches its topics.
pub fn name_event(interface: &Value, topics: &[Value], data: &Value) -> Option<(String, Value)> {
    for ev in interface["events"].as_array()? {
        let prefix: Vec<&str> = ev["prefix_topics"].as_array()?.iter().filter_map(Value::as_str).collect();
        if topics.len() < prefix.len() || !prefix.iter().zip(topics).all(|(p, t)| t.as_str() == Some(p)) {
            continue;
        }
        let params = ev["params"].as_array()?;
        let topic_params: Vec<&Value> = params.iter().filter(|p| p["location"] == "topic").collect();
        let data_params: Vec<&Value> = params.iter().filter(|p| p["location"] == "data").collect();
        if topics.len() != prefix.len() + topic_params.len() {
            continue;
        }
        let mut fields = Map::new();
        for (p, t) in topic_params.iter().zip(&topics[prefix.len()..]) {
            fields.insert(p["name"].as_str().unwrap_or("?").into(), t.clone());
        }
        match ev["data_format"].as_str() {
            Some("map") => {
                for p in &data_params {
                    let name = p["name"].as_str().unwrap_or("?");
                    fields.insert(name.into(), data.get(name).cloned().unwrap_or(Value::Null));
                }
            }
            Some("vec") => {
                for (i, p) in data_params.iter().enumerate() {
                    fields.insert(p["name"].as_str().unwrap_or("?").into(), data.get(i).cloned().unwrap_or(Value::Null));
                }
            }
            _ => {
                if let Some(p) = data_params.first() {
                    fields.insert(p["name"].as_str().unwrap_or("?").into(), data.clone());
                }
            }
        }
        return Some((ev["name"].as_str().unwrap_or("?").to_string(), Value::Object(fields)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::{
        ScSpecEventParamV0, ScSpecEventV0, ScSpecFunctionInputV0, ScSpecFunctionV0, ScSymbol, StringM, VecM,
    };

    fn leb(mut n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    /// A minimal WASM module with one custom section.
    fn wasm_with(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut body = leb(name.len());
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(payload);
        let mut w = b"\0asm\x01\0\0\0".to_vec();
        // An unrelated (type) section first, which must be skipped.
        w.extend_from_slice(&[1, 1, 0]);
        w.push(0);
        w.extend(leb(body.len()));
        w.extend(body);
        w
    }

    fn sample_spec() -> Vec<u8> {
        let swap = ScSpecEntry::FunctionV0(ScSpecFunctionV0 {
            doc: StringM::default(),
            name: ScSymbol("swap".try_into().unwrap()),
            inputs: VecM::try_from(vec![
                ScSpecFunctionInputV0 { doc: StringM::default(), name: "user".try_into().unwrap(), type_: ScSpecTypeDef::Address },
                ScSpecFunctionInputV0 { doc: StringM::default(), name: "amount_in".try_into().unwrap(), type_: ScSpecTypeDef::I128 },
            ])
            .unwrap(),
            outputs: VecM::try_from(vec![ScSpecTypeDef::I128]).unwrap(),
        });
        let swapped = ScSpecEntry::EventV0(ScSpecEventV0 {
            doc: StringM::default(),
            lib: StringM::default(),
            name: "Swapped".try_into().unwrap(),
            prefix_topics: VecM::try_from(vec![ScSymbol("swapped".try_into().unwrap())]).unwrap(),
            params: VecM::try_from(vec![
                ScSpecEventParamV0 { doc: StringM::default(), name: "user".try_into().unwrap(), type_: ScSpecTypeDef::Address, location: ScSpecEventParamLocationV0::TopicList },
                ScSpecEventParamV0 { doc: StringM::default(), name: "amount_in".try_into().unwrap(), type_: ScSpecTypeDef::I128, location: ScSpecEventParamLocationV0::Data },
                ScSpecEventParamV0 { doc: StringM::default(), name: "amount_out".try_into().unwrap(), type_: ScSpecTypeDef::I128, location: ScSpecEventParamLocationV0::Data },
            ])
            .unwrap(),
            data_format: ScSpecEventDataFormat::Map,
        });
        let mut out = swap.to_xdr(Limits::none()).unwrap();
        out.extend(swapped.to_xdr(Limits::none()).unwrap());
        out
    }

    #[test]
    fn reads_the_interface_from_wasm() {
        let wasm = wasm_with("contractspecv0", &sample_spec());
        let entries = spec_entries(&wasm);
        assert_eq!(entries.len(), 2);
        let d = describe(&entries);
        assert_eq!(d["functions"][0]["name"], "swap");
        assert_eq!(d["functions"][0]["inputs"][1], json!({ "name": "amount_in", "type": "i128" }));
        assert_eq!(d["functions"][0]["outputs"], json!(["i128"]));
        assert_eq!(d["events"][0]["prefix_topics"], json!(["swapped"]));
        assert_eq!(d["events"][0]["data_format"], "map");
        assert!(spec_entries(b"not wasm").is_empty());
        assert!(spec_entries(&wasm_with("other", b"x")).is_empty());
    }

    #[test]
    fn names_calls_and_events() {
        let d = describe(&spec_entries(&wasm_with("contractspecv0", &sample_spec())));
        assert_eq!(name_call(&d, "swap", &[json!("GA..."), json!("5")]), json!({ "user": "GA...", "amount_in": "5" }));
        // Unknown function or wrong arity: positional.
        assert_eq!(name_call(&d, "other", &[json!(1)]), json!([1]));
        let (name, fields) = name_event(&d, &[json!("swapped"), json!("GU...")], &json!({ "amount_in": "5", "amount_out": "7" })).unwrap();
        assert_eq!(name, "Swapped");
        assert_eq!(fields, json!({ "user": "GU...", "amount_in": "5", "amount_out": "7" }));
        assert!(name_event(&d, &[json!("other")], &Value::Null).is_none());
        assert!(name_event(&d, &[json!("swapped")], &Value::Null).is_none(), "missing topic param");
    }

    #[test]
    fn types_read_like_rust() {
        let t = ScSpecTypeDef::Vec(Box::new(stellar_xdr::ScSpecTypeVec {
            element_type: Box::new(ScSpecTypeDef::Option(Box::new(stellar_xdr::ScSpecTypeOption { value_type: Box::new(ScSpecTypeDef::Address) }))),
        }));
        assert_eq!(type_name(&t), "Vec<Option<Address>>");
    }
}

#[cfg(test)]
mod live {
    /// Reads real interfaces from mainnet: `cargo test -- --ignored live`.
    #[tokio::test]
    #[ignore]
    async fn reads_mainnet_interfaces() {
        let rpc = crate::services::soroban_rpc::SorobanRpc::new();
        let kale = super::fetch(&rpc, "CDL74RF5BLYR2YBLCCI7F5FB6TPSCLKEJUBSD2RSVWZ4YHF3VMFAIGWA").await.unwrap();
        assert_eq!(kale["kind"], "wasm");
        let names: Vec<&str> = kale["functions"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
        assert!(names.contains(&"plant") && names.contains(&"work"), "{names:?}");
        println!("KALE: {}", serde_json::to_string(&kale["functions"]).unwrap().chars().take(600).collect::<String>());
        println!("KALE meta: {} events: {}", kale["meta"], kale["events"].as_array().unwrap().len());
        let xlm = super::fetch(&rpc, "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA").await.unwrap();
        assert_eq!(xlm["kind"], "stellar_asset");
        println!("XLM SAC asset: {}", xlm["asset"]);
    }
}
