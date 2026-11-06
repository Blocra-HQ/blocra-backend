//! BlocRA attestations: reports signed with BlocRA's Ed25519 key, so a
//! report's figures can be checked without trusting whoever passes it on.
//!
//! The signature covers the exact bytes of `report_json` (the report as
//! compact JSON), so verifiers never have to re-serialise anything: they
//! check the signature over that string, then parse it. The public key is a
//! Stellar account id (G...), published at GET /api/attestation/key, and the
//! sha256 of `report_json` is what goes into the on-chain report registry.
//!
//! Configure with BLOCRA_SIGNING_SECRET, a Stellar secret seed (S...),
//! e.g. from `stellar keys generate blocra-signer && stellar keys show blocra-signer`.
//! The key never needs funds; it only signs.

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Version of the report methodology; bumped whenever a definition changes,
/// so verifiers can tell which rules produced a report.
pub const METHOD_VERSION: &str = "traction/1";

#[derive(Clone)]
pub struct Attestor {
    key: Option<SigningKey>,
}

impl Attestor {
    pub fn from_env() -> Self {
        let key = std::env::var("BLOCRA_SIGNING_SECRET").ok().and_then(|s| {
            match stellar_strkey::ed25519::PrivateKey::from_string(s.trim()) {
                Ok(k) => Some(SigningKey::from_bytes(&k.0)),
                Err(_) => {
                    log::error!("BLOCRA_SIGNING_SECRET is not a valid Stellar secret seed (S...); reports will be unsigned");
                    None
                }
            }
        });
        Self { key }
    }

    #[cfg(test)]
    pub fn with_seed(seed: [u8; 32]) -> Self {
        Self { key: Some(SigningKey::from_bytes(&seed)) }
    }

    pub fn is_configured(&self) -> bool {
        self.key.is_some()
    }

    /// The signer's public key as a Stellar account id (G...).
    pub fn signer(&self) -> Option<String> {
        self.key.as_ref().map(|k| {
            stellar_strkey::ed25519::PublicKey(k.verifying_key().to_bytes()).to_string().as_str().to_string()
        })
    }

    /// Signs a report: returns the envelope `{report_json, attestation}`.
    pub fn sign(&self, report: &Value) -> Option<Value> {
        let key = self.key.as_ref()?;
        let report_json = serde_json::to_string(report).ok()?;
        let signature = key.sign(report_json.as_bytes());
        let digest = Sha256::digest(report_json.as_bytes());
        Some(json!({
            "report_json": report_json,
            "attestation": {
                "algorithm": "ed25519",
                "signer": self.signer(),
                "signature": hex(&signature.to_bytes()),
                "payload_sha256": hex(&digest),
                "method_version": METHOD_VERSION,
                "signed_at": chrono::Utc::now().to_rfc3339(),
            }
        }))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    #[test]
    fn signs_the_exact_report_bytes() {
        let a = Attestor::with_seed([7; 32]);
        let env = a.sign(&json!({ "b": 1, "a": [1.5, "x"] })).unwrap();
        let text = env["report_json"].as_str().unwrap();
        let att = &env["attestation"];
        assert_eq!(att["method_version"], METHOD_VERSION);
        assert!(att["signer"].as_str().unwrap().starts_with('G'));

        // Verify the way an outside verifier would: strkey -> key, hex -> signature.
        let pk = stellar_strkey::ed25519::PublicKey::from_string(att["signer"].as_str().unwrap()).unwrap();
        let vk = VerifyingKey::from_bytes(&pk.0).unwrap();
        let sig_hex = att["signature"].as_str().unwrap();
        let sig_bytes: Vec<u8> = (0..sig_hex.len()).step_by(2).map(|i| u8::from_str_radix(&sig_hex[i..i + 2], 16).unwrap()).collect();
        let sig = Signature::from_slice(&sig_bytes).unwrap();
        assert!(vk.verify(text.as_bytes(), &sig).is_ok());
        // Any change to the text breaks it.
        assert!(vk.verify(text.replace('1', "2").as_bytes(), &sig).is_err());
        assert_eq!(att["payload_sha256"].as_str().unwrap(), hex(&Sha256::digest(text.as_bytes())));
    }

    #[test]
    fn unconfigured_signs_nothing() {
        let a = Attestor { key: None };
        assert!(!a.is_configured());
        assert!(a.sign(&json!({})).is_none());
    }
}
