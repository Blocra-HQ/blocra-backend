-- One-time Stellar wallet sign-in challenges (see src/auth/stellar_wallet.rs).
CREATE TABLE IF NOT EXISTS wallet_challenges (
    nonce      TEXT PRIMARY KEY,
    account    TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_wallet_challenges_expires ON wallet_challenges(expires_at);
