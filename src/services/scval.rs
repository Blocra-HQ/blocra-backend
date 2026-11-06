//! Just enough Stellar XDR to read contract-call parameters from Horizon:
//! ScVal addresses (to G.../C... strkeys) and symbols.

use base64::{engine::general_purpose::STANDARD, Engine};

const SCV_SYMBOL: u32 = 15;
const SCV_ADDRESS: u32 = 18;
const SC_ADDRESS_ACCOUNT: u32 = 0;
const SC_ADDRESS_CONTRACT: u32 = 1;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A base64 ScVal holding an address, as a strkey (C... or G...).
pub fn address(b64: &str) -> Option<String> {
    let b = STANDARD.decode(b64).ok()?;
    if u32_at(&b, 0)? != SCV_ADDRESS {
        return None;
    }
    match u32_at(&b, 4)? {
        SC_ADDRESS_CONTRACT => {
            let id: [u8; 32] = b.get(8..40)?.try_into().ok()?;
            Some(stellar_strkey::Contract(id).to_string().as_str().to_string())
        }
        SC_ADDRESS_ACCOUNT => {
            // PublicKey union: type 0 = ed25519, then 32 bytes.
            if u32_at(&b, 8)? != 0 {
                return None;
            }
            let key: [u8; 32] = b.get(12..44)?.try_into().ok()?;
            Some(stellar_strkey::ed25519::PublicKey(key).to_string().as_str().to_string())
        }
        _ => None,
    }
}

/// A base64 ScVal holding a symbol, as text.
pub fn symbol(b64: &str) -> Option<String> {
    let b = STANDARD.decode(b64).ok()?;
    if u32_at(&b, 0)? != SCV_SYMBOL {
        return None;
    }
    let len = u32_at(&b, 4)? as usize;
    if len > 32 {
        return None;
    }
    String::from_utf8(b.get(8..8 + len)?.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_horizon_parameters() {
        // From a mainnet invoke_host_function operation (2026-11-06).
        let contract = address("AAAAEgAAAAHtzg5P6SASi0gXcoiUXbIyNil74ihUeLn00E5zwvYBPg==").unwrap();
        assert!(contract.starts_with('C') && contract.len() == 56, "{contract}");
        assert_eq!(symbol("AAAADwAAAAlzZXRfcHJpY2UAAAA=").as_deref(), Some("set_price"));
        let account = address("AAAAEgAAAAAAAAAARtF7MiPbh3kx+zG86gAmnWnZn9jMv7pcjP+6o6HMDaY=").unwrap();
        assert!(account.starts_with('G') && account.len() == 56, "{account}");
        assert_eq!(symbol("AAAAEgAAAAHtzg5P6SASi0gXcoiUXbIyNil74ihUeLn00E5zwvYBPg=="), None);
    }
}
