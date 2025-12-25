-- Create rewards table
CREATE TABLE IF NOT EXISTS rewards (
    id TEXT PRIMARY KEY,
    bounty_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    amount REAL NOT NULL,
    token TEXT NOT NULL,
    transaction_hash TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (bounty_id) REFERENCES bounties(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_rewards_bounty ON rewards(bounty_id);
CREATE INDEX idx_rewards_user ON rewards(user_id);
CREATE INDEX idx_rewards_status ON rewards(status);
