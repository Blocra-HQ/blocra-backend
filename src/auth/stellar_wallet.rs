//! Stellar wallet sign-in.
//!
//! Adapted from the SEP-10-style flow in stellar-insights
//! (`backend/src/auth/sep10_simple.rs`): the server issues a one-time challenge
//! for an account, the wallet signs it (Freighter's `signMessage`, SEP-53),
//! and the server checks the Ed25519 signature before issuing a BlocRA JWT.
//! Challenges live in SQLite (`wallet_challenges`) instead of Redis, are valid
//! for five minutes, and can be used once.

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::db::DbPool;

/// How long a challenge stays valid.
pub const CHALLENGE_EXPIRY_SECONDS: i64 = 300;

const CHALLENGE_HEADER: &str = "BlocRA wants you to sign in with your Stellar account:";

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
    pub account: String,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    /// Sign these exact UTF-8 bytes with the account's key.
    pub challenge: String,
    pub expires_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct VerificationRequest {
    /// The challenge exactly as `create_challenge` returned it.
    pub challenge: String,
    /// Base64 Ed25519 signature over the challenge (raw or SEP-53 digest).
    pub signature: String,
}

/// A `G...` account: 56 chars with a valid StrKey checksum.
pub fn is_valid_account(account: &str) -> bool {
    stellar_strkey::ed25519::PublicKey::from_string(account).is_ok()
}

fn new_nonce() -> String {
    // Two v4 UUIDs = 244 random bits from the OS RNG.
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn challenge_text(account: &str, nonce: &str, issued_at: i64, expires_at: i64) -> String {
    format!(
        "{CHALLENGE_HEADER}\n{account}\n\nNonce: {nonce}\nIssued At: {issued_at}\nExpires At: {expires_at}"
    )
}

/// Pulls (account, nonce, expires_at) back out of a challenge.
fn parse_challenge(challenge: &str) -> Result<(String, String, i64)> {
    let mut lines = challenge.lines();
    if lines.next() != Some(CHALLENGE_HEADER) {
        return Err(anyhow!("Not a BlocRA sign-in challenge"));
    }
    let account = lines.next().ok_or_else(|| anyhow!("Missing account"))?.to_string();
    let mut nonce = None;
    let mut expires_at = None;
    for line in lines {
        if let Some(v) = line.strip_prefix("Nonce: ") {
            nonce = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("Expires At: ") {
            expires_at = v.parse::<i64>().ok();
        }
    }
    Ok((
        account,
        nonce.ok_or_else(|| anyhow!("Missing nonce"))?,
        expires_at.ok_or_else(|| anyhow!("Missing expiry"))?,
    ))
}

/// Issues a one-time challenge for `account` and remembers its nonce.
pub async fn create_challenge(pool: &DbPool, account: &str) -> Result<ChallengeResponse> {
    if !is_valid_account(account) {
        return Err(anyhow!("Invalid Stellar account address"));
    }
    let now = Utc::now().timestamp();
    let expires_at = now + CHALLENGE_EXPIRY_SECONDS;
    let nonce = new_nonce();

    // Drop stale challenges so the table can't grow without bound.
    sqlx::query("DELETE FROM wallet_challenges WHERE expires_at < ?")
        .bind(now)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO wallet_challenges (nonce, account, expires_at) VALUES (?, ?, ?)")
        .bind(&nonce)
        .bind(account)
        .bind(expires_at)
        .execute(pool)
        .await?;

    Ok(ChallengeResponse {
        challenge: challenge_text(account, &nonce, now, expires_at),
        expires_at,
    })
}

/// Checks the signature and consumes the challenge. Returns the verified
/// account; the caller signs the user in as it.
pub async fn verify_challenge(pool: &DbPool, request: &VerificationRequest) -> Result<String> {
    let (account, nonce, expires_at) = parse_challenge(&request.challenge)?;

    verify_account_signature(&account, request.challenge.as_bytes(), Some(&request.signature))?;

    if Utc::now().timestamp() > expires_at {
        return Err(anyhow!("Challenge expired"));
    }

    // Consume: a challenge signs in at most once, and only for its account.
    let consumed = sqlx::query(
        "DELETE FROM wallet_challenges WHERE nonce = ? AND account = ? AND expires_at >= ?",
    )
    .bind(&nonce)
    .bind(&account)
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await?;
    if consumed.rows_affected() != 1 {
        return Err(anyhow!("Challenge not found or already used"));
    }

    Ok(account)
}

/// Check that `signature` (base64) is the Ed25519 signature of `message` by
/// the Stellar account `account` (a `G...` public key).
pub fn verify_account_signature(
    account: &str,
    message: &[u8],
    signature: Option<&str>,
) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let signature = signature.ok_or_else(|| anyhow!("Missing challenge signature"))?;
    let signature_bytes: [u8; 64] = BASE64
        .decode(signature)
        .map_err(|_| anyhow!("Invalid signature encoding"))?
        .try_into()
        .map_err(|_| anyhow!("Invalid signature length"))?;

    let public_key = stellar_strkey::ed25519::PublicKey::from_string(account)
        .map_err(|_| anyhow!("Invalid account address"))?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key.0).map_err(|_| anyhow!("Invalid account key"))?;

    let signature = Signature::from_bytes(&signature_bytes);

    // Wallets that implement SEP-53 (Freighter's signMessage) sign
    // SHA-256("Stellar Signed Message:\n" + message) rather than the raw
    // bytes. Accept either form; both prove control of the account key.
    let sep53_digest = {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"Stellar Signed Message:\n");
        hasher.update(message);
        hasher.finalize()
    };

    if verifying_key.verify(message, &signature).is_ok()
        || verifying_key.verify(&sep53_digest, &signature).is_ok()
    {
        Ok(())
    } else {
        Err(anyhow!("Challenge signature does not match the account"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_and_key(seed: u8) -> (String, ed25519_dalek::SigningKey) {
        let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        let account = stellar_strkey::ed25519::PublicKey(key.verifying_key().to_bytes())
            .to_string()
            .to_string();
        (account, key)
    }

    fn sign(key: &ed25519_dalek::SigningKey, message: &[u8]) -> String {
        use ed25519_dalek::Signer;
        BASE64.encode(key.sign(message).to_bytes())
    }

    #[test]
    fn accepts_the_accounts_own_signature() {
        let (account, key) = account_and_key(7);
        let challenge = b"challenge-bytes";
        let signature = sign(&key, challenge);

        assert!(verify_account_signature(&account, challenge, Some(&signature)).is_ok());
    }

    #[test]
    fn accepts_a_sep53_signed_message() {
        use sha2::Digest;
        let (account, key) = account_and_key(7);
        let challenge = b"challenge-bytes";
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"Stellar Signed Message:\n");
        hasher.update(challenge);
        let signature = sign(&key, &hasher.finalize());

        assert!(verify_account_signature(&account, challenge, Some(&signature)).is_ok());
    }

    #[test]
    fn rejects_a_missing_signature() {
        let (account, _) = account_and_key(7);
        assert!(verify_account_signature(&account, b"challenge", None).is_err());
    }

    #[test]
    fn rejects_a_signature_from_another_account() {
        let (account, _) = account_and_key(7);
        let (_, attacker) = account_and_key(9);
        let challenge = b"challenge-bytes";
        let forged = sign(&attacker, challenge);

        assert!(verify_account_signature(&account, challenge, Some(&forged)).is_err());
    }

    #[test]
    fn rejects_a_signature_over_a_different_challenge() {
        let (account, key) = account_and_key(7);
        let signature = sign(&key, b"an older challenge");

        assert!(verify_account_signature(&account, b"this challenge", Some(&signature)).is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        let (account, _) = account_and_key(7);
        assert!(verify_account_signature(&account, b"c", Some("not base64!")).is_err());
        assert!(verify_account_signature(&account, b"c", Some(&BASE64.encode([0u8; 10]))).is_err());
        assert!(
            verify_account_signature("GNOTAKEY", b"c", Some(&BASE64.encode([0u8; 64]))).is_err()
        );
    }

    async fn pool() -> DbPool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        pool
    }

    #[actix_rt::test]
    async fn signs_in_once_with_a_signed_challenge() {
        let pool = pool().await;
        let (account, key) = account_and_key(7);
        let issued = create_challenge(&pool, &account).await.unwrap();

        let request = VerificationRequest {
            signature: sign(&key, issued.challenge.as_bytes()),
            challenge: issued.challenge,
        };
        assert_eq!(verify_challenge(&pool, &request).await.unwrap(), account);
        // Replaying the same signed challenge must fail.
        assert!(verify_challenge(&pool, &request).await.is_err());
    }

    #[actix_rt::test]
    async fn rejects_someone_else_signing_the_challenge() {
        let pool = pool().await;
        let (account, _) = account_and_key(7);
        let (_, attacker) = account_and_key(9);
        let issued = create_challenge(&pool, &account).await.unwrap();
        let request = VerificationRequest {
            signature: sign(&attacker, issued.challenge.as_bytes()),
            challenge: issued.challenge,
        };
        assert!(verify_challenge(&pool, &request).await.is_err());
    }

    #[actix_rt::test]
    async fn rejects_invalid_accounts_and_unknown_nonces() {
        let pool = pool().await;
        assert!(create_challenge(&pool, "INVALID").await.is_err());

        // A correctly signed challenge the server never issued.
        let (account, key) = account_and_key(7);
        let forged = challenge_text(&account, "made-up", 0, Utc::now().timestamp() + 60);
        let request = VerificationRequest {
            signature: sign(&key, forged.as_bytes()),
            challenge: forged,
        };
        assert!(verify_challenge(&pool, &request).await.is_err());
    }

    #[test]
    fn challenge_round_trips() {
        let text = challenge_text("GABC", "n0nce", 10, 310);
        assert_eq!(
            parse_challenge(&text).unwrap(),
            ("GABC".to_string(), "n0nce".to_string(), 310)
        );
        assert!(parse_challenge("hello\nGABC").is_err());
    }

    #[test]
    fn validates_account_checksums() {
        let (account, _) = account_and_key(7);
        assert!(is_valid_account(&account));
        let mut broken = account.clone();
        broken.replace_range(55..56, if account.ends_with('A') { "B" } else { "A" });
        assert!(!is_valid_account(&broken));
        assert!(!is_valid_account("0x07070d915635269ea0930fa1c538f2d026e02e5078884aeb007141c39f481eee"));
    }
}
